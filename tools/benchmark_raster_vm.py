#!/usr/bin/env python3
"""Measure frozen real raster calls using GWemu baseline instruction counters.

Disposable headless VM only. In-guest DWT endpoints and an empty-call overhead
are calibrated against a full manually stepped call. Debugger-endpoint deltas
are recorded but are not measurements. Masks IRQs in this isolated kernel
cohort, restores inputs/outputs/ABI, and includes dispatch and math helpers.
These are retired instruction counts, not M7 cycles, cache costs or hardware FPS.
"""
import argparse
import hashlib
import json
from pathlib import Path
import struct
import subprocess
import time

import numpy as np
from elftools.elf.elffile import ELFFile
from gwprov.debug_shell import DebugSession, GwemuGDBBackend
from gwprov.gwemu_manager import start_instance
from gwprov.daemon import managed_instances, qmp_request
from replay_game_frames import run_to
from audit_itcm_vm import audit_loaded_image

DWT_CTRL, DWT_COUNT = 0xe0001000, 0xe0001004
REGISTERS = [f'r{i}' for i in range(13)] + ['sp', 'lr', 'pc', 'xpsr']


def coverage(raw):
    _, mode, _, *words = struct.unpack('<HBB7I', raw)
    xy = np.array([struct.unpack('<hh', struct.pack('<I', w)) for w in words[:3]], dtype=np.int64)
    a, b, c = xy
    area = int((b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]))
    if not area:
        return 0
    yy, xx = np.mgrid[:120, :160]
    xx = xx * 16 + 8
    yy = yy * 16 + 8
    inside = np.ones(xx.shape, dtype=bool)
    for p, q in zip(xy, np.roll(xy, -1, axis=0)):
        w = (q[0] - p[0]) * (yy - p[1]) - (q[1] - p[1]) * (xx - p[0])
        inside &= w >= 0 if area > 0 else w <= 0
    return int(inside.sum())


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ['profile', 'elf', 'output']:
        parser.add_argument(name, type=Path)
    parser.add_argument('--stage-index', type=int, choices=range(10), default=7)
    parser.add_argument('--wrapper', type=Path, required=True, help='Docker-assembled raster_counter_wrapper.o (ET_REL)')
    parser.add_argument('--reference-image',action='store_true',help='Measure release defaults without toggling a raster control')
    parser.add_argument('--unsigned-depth-sample', action='store_true',
                        help='Also capture a real depth numerator fitting unsigned32 but not signed32')
    parser.add_argument('--control',choices=['opaque','convert','coverage'],default='opaque',help='Same-build raster control; reference-image leaves existing defaults fixed')
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    report = dict(note=__doc__.strip(), samples=[], stage=args.stage_index,
                  reference_image=args.reference_image,
                  control='fixed' if args.reference_image else args.control,
                  elf_sha256=hashlib.sha256(args.elf.read_bytes()).hexdigest())
    pid = None
    backend = None

    def save_report():
        (args.output / 'report.json').write_text(json.dumps(report, indent=2) + '\n')

    try:
        start_instance(str(args.profile.resolve()), headless=True, gdb_port=1236,
                       start_halted=True, rtc_epoch=1700000000,
                       timing_mode='baseline', pacing='unthrottled')
        row = next(x for x in managed_instances() if x['profile'] == str(args.profile.resolve()))
        pid = row['pid']
        report['cohort'] = row
        assert row['timing']['mode'] == 'baseline'
        assert row['timing']['cycleSemantics'] == 'one-instruction-one-cycle'
        save_report()
        backend = GwemuGDBBackend(port=1236)
        for attempt in range(100):
            try:
                backend.open(halt=True)
                break
            except Exception:
                backend.close()
                if attempt == 99:
                    raise
                time.sleep(.05)
        dbg = DebugSession(backend, 'gwemu', qmp_socket=f'gwprov://{pid}')
        dbg.symbols.load(args.elf)
        backend.write_memory(0x58004500, (0x544f4f42).to_bytes(4, 'little'))
        hit = run_to(dbg, 'smg_xip_load')
        run_to(dbg, hit['registers']['lr'])
        dbg.symbols.rebase_from_runtime_pointers(backend.read_memory)
        api = run_to(dbg, 'smg_game_frame')['registers']
        report['itcm'] = audit_loaded_image(dbg, backend.read_memory, args.elf)
        report['memory'] = dbg.read_path('smg_memory_stats')
        # This preserved firmware starts with DWT disabled. Enable counting
        # only in this disposable VM, retaining the initial controls and never
        # resetting CYCCNT. No hardware/debug-session default is changed.
        demcr = int.from_bytes(backend.read_memory(0xe000edfc, 4), 'little')
        dwt_ctrl = int.from_bytes(backend.read_memory(DWT_CTRL, 4), 'little')
        backend.write_memory(0xe000edfc, (demcr | (1 << 24)).to_bytes(4, 'little'))
        backend.write_memory(DWT_CTRL, (dwt_ctrl | 1).to_bytes(4, 'little'))
        report['dwt_enable'] = dict(demcr_before=demcr, ctrl_before=dwt_ctrl,
            demcr_after=int.from_bytes(backend.read_memory(0xe000edfc, 4), 'little'),
            ctrl_after=int.from_bytes(backend.read_memory(DWT_CTRL, 4), 'little'))
        assert report['dwt_enable']['demcr_after'] & (1 << 24)
        assert report['dwt_enable']['ctrl_after'] & 1
        with args.wrapper.open('rb') as stream:
            obj = ELFFile(stream)
            wrapper = obj.get_section_by_name('.text').data()
            symbols = obj.get_section_by_name('.symtab')
            wrapper_offset = symbols.get_symbol_by_name('raster_counter_call')[0]['st_value'] & ~1
            empty_offset = symbols.get_symbol_by_name('raster_counter_empty')[0]['st_value'] & ~1
            assert obj.get_section_by_name('.rel.text') is None, 'Wrapper must be self-contained'
        # Resolve the SDK allocator from its actual DWARF member layout and
        # the running firmware ABI; never reset or reclaim a firmware pool.
        mem_ctl_offset = None
        with args.elf.open('rb') as stream:
            dwarf = ELFFile(stream).get_dwarf_info(relocate_dwarf_sections=False)
            for cu in dwarf.iter_CUs():
                for die in cu.iter_DIEs():
                    if die.tag == 'DW_TAG_typedef' and die.attributes.get('DW_AT_name') and die.attributes['DW_AT_name'].value == b'gw_firmware_abi_t':
                        for member in die.get_DIE_from_attribute('DW_AT_type').iter_children():
                            if member.attributes.get('DW_AT_name') and member.attributes['DW_AT_name'].value == b'mem_ctl':
                                mem_ctl_offset = member.attributes['DW_AT_data_member_location'].value
                        break
                if mem_ctl_offset is not None:
                    break
        assert isinstance(mem_ctl_offset, int)
        firmware_base = int.from_bytes(backend.read_memory(0xe000ed08, 4), 'little')
        abi = firmware_base + 0x400
        version, size = struct.unpack('<2I', backend.read_memory(abi, 8))
        assert version == 2 and size >= mem_ctl_offset + 4
        mem_ctl = int.from_bytes(backend.read_memory(abi + mem_ctl_offset, 4), 'little')
        for name, value in [('r0', 0), ('r1', 0), ('r2', 1), ('r3', 160),
                            ('lr', api['pc'] | 1), ('pc', mem_ctl & ~1)]:
            backend.write_register(name, value)
        run_to(dbg, api['pc'])
        wrapper_address = backend.read_register('r0')
        assert report['itcm']['bytes'] <= wrapper_address < wrapper_address + 160 <= 65536
        assert len(wrapper) <= 128
        backend.write_memory(wrapper_address, wrapper)
        assert backend.read_memory(wrapper_address, len(wrapper)) == wrapper
        result_address = wrapper_address + 128
        report['wrapper'] = dict(address=wrapper_address, allocated_bytes=160, code_bytes=len(wrapper),
            sha256=hashlib.sha256(wrapper).hexdigest(), mem_ctl_offset=mem_ctl_offset,
            mem_ctl=mem_ctl, scope='Owned SDK ITCM allocation in disposable VM; released with the VM')
        for name, value in [('r0', args.stage_index), ('r1', 0), ('r2', 1),
                            ('lr', api['pc'] | 1), ('pc', dbg.at('game_load_stage') & ~1)]:
            backend.write_register(name, value)
        run_to(dbg, api['pc'])
        for name in REGISTERS:
            backend.write_register(name, api[name])
        for name, value in [('r0',0),('r1',8),('r3',1)]:
            backend.write_register(name,value)
        report['first_api_override']={'buttons':0,'dt_q8':8,'render':True}
        for name in ['g_depth_enabled', 'g_perspective_enabled']:
            backend.write_memory(dbg.at(name), b'\1')
        assert dbg.read_path('smg_render_stats')['raster_width'] == 160
        classes = {'small': (1, 16), 'medium': (64, 256), 'large': (512, 8192)}
        if args.unsigned_depth_sample:
            classes['unsigned-depth'] = None
        collected = set()
        for candidate in range(4096):
            hit = run_to(dbg, 'engine_raster_inner')
            entry = hit['registers']
            raw = backend.read_memory(entry['r0'], 32)
            mode = raw[2]
            pixels = coverage(raw) if mode == 1 else 0
            label = next((k for k, bounds in classes.items()
                          if bounds is not None and k not in collected
                          and bounds[0] <= pixels <= bounds[1]), None)
            if args.unsigned_depth_sample and mode == 1 and pixels and 'small' in collected and 'unsigned-depth' not in collected:
                xy = [struct.unpack_from('<hh', raw, 4 + 4 * i) for i in range(3)]
                q = struct.unpack('<3I', backend.read_memory(dbg.read_path('g_raster_depth'), 12))
                x, y = zip(*xy)
                area = (x[1]-x[0])*(y[2]-y[0])-(y[1]-y[0])*(x[2]-x[0])
                d1, d2 = q[1]-q[0], q[2]-q[0]
                numerators = [(d1*(y[2]-y[0])-d2*(y[1]-y[0]))*4096,
                              ((x[1]-x[0])*d2-(x[2]-x[0])*d1)*4096]
                if 0 < abs(area) <= 0xffffffff and any(abs(n) <= 0xffffffff and not -0x80000000 <= n <= 0x7fffffff for n in numerators):
                    label = 'unsigned-depth'
            if not collected and label != 'small':
                label = None  # Calibrate on the complete small call first.
            if label is None:
                run_to(dbg, entry['lr'])
                continue
            print('captured', label, pixels, 'pixels', flush=True)
            collected.add(label)
            framebuffer = dbg.read_path('g_engine_fb')
            assert framebuffer == dbg.at('smg_framebuffer')
            outputs = [(name, dbg.at(name), dbg.memory_path(name)['size']) for name in
                       ['smg_framebuffer', 'g_depth_buffer', 'smg_geometry_stats']]
            saved = [(name, address, backend.read_memory(address, size)) for name, address, size in outputs]
            depth_ptr = dbg.read_path('g_raster_depth')
            depths = backend.read_memory(depth_ptr, 12)
            assert all(struct.unpack('<3I', depths))
            # Preserve the actual firmware's stack for calibration/reset;
            # this is debug restoration, not reclaiming reserved stack RAM.
            assert 0x2001b020 < entry['sp'] <= 0x20020000
            stack_address = 0x2001b020
            stack = backend.read_memory(stack_address, entry['sp'] - stack_address)
            initial_controls = {key: dbg.read_path(key) for key in ['g_depth_active','g_perspective_active']}
            for key in ['g_opaque_raster_enabled','g_raster_convert_narrow_enabled','g_raster_coverage_narrow_enabled']:
                try:initial_controls[key]=dbg.read_path(key)
                except (ValueError,KeyError):pass
            control_key={'opaque':'g_opaque_raster_enabled','convert':'g_raster_convert_narrow_enabled','coverage':'g_raster_coverage_narrow_enabled'}[args.control]
            if not args.reference_image:
                assert control_key in initial_controls,('Missing requested raster control',control_key)
            # QEMU's advertised PRIMASK register acknowledges GDB writes but
            # this binary reads the old value back. Use verified NVIC MMIO
            # masks instead; do not silently assume a register write worked.
            enabled_irqs = backend.read_memory(0xe000e100, 32)
            systick_ctrl = int.from_bytes(backend.read_memory(0xe000e010, 4), 'little') & 7
            pending = int.from_bytes(backend.read_memory(0xe000ed04, 4), 'little')
            assert not pending & (1 << 28), 'Pending PendSV at raster entry'
            backend.write_memory(0xe000e180, b'\xff' * 32)
            backend.write_memory(0xe000e010, (systick_ctrl & ~2).to_bytes(4, 'little'))
            backend.write_memory(0xe000ed04, ((1 << 27) | (1 << 25)).to_bytes(4, 'little'))
            assert backend.read_memory(0xe000e100, 32) == bytes(32)
            assert not int.from_bytes(backend.read_memory(0xe000e010, 4), 'little') & 2
            assert not (entry['xpsr'] & 0x1ff), 'Raster entry is inside an exception'
            fpscr = backend.read_register('fpscr') if 'fpscr' in backend.register_numbers else None

            def read_u32(address):
                return int.from_bytes(backend.read_memory(address, 4), 'little')

            def restore(controls):
                for _, address, data in saved:
                    backend.write_memory(address, data)
                backend.write_memory(stack_address, stack)
                backend.write_memory(depth_ptr, depths)
                backend.write_memory(entry['r0'], raw)
                for key, value in controls.items():
                    backend.write_memory(dbg.at(key), bytes([value]))
                backend.write_memory(0xe000ed04, ((1 << 27) | (1 << 25)).to_bytes(4, 'little'))
                if fpscr is not None:
                    backend.write_register('fpscr', fpscr)
                for name in REGISTERS:
                    backend.write_register(name, entry[name])

            def result():
                return {name: hashlib.sha256(backend.read_memory(address, size)).hexdigest()
                        for name, address, size in outputs}

            call = dict(label=label, candidate=candidate, covered_pixels=pixels,
                        entry=list(struct.unpack('<8I', raw)), depths=list(struct.unpack('<3I', depths)),
                        initial_output_hashes={name: hashlib.sha256(data).hexdigest() for name, _, data in saved},
                        controls=initial_controls, irq_mask=dict(iser_before=enabled_irqs.hex(),
                            systick_ctrl_before=systick_ctrl, icsr_before=pending,
                            iser_masked=backend.read_memory(0xe000e100, 32).hex()), measurements=[])
            texture_ref=struct.unpack_from('<I',raw,28)[0]
            mips=struct.unpack('<2I',backend.read_memory(texture_ref,8))
            call['canonical_input_hashes']={'screen_uv':hashlib.sha256(raw[4:28]).hexdigest(),
                'depth':hashlib.sha256(depths).hexdigest(),
                **{f'mip{i}':hashlib.sha256(backend.read_memory(address,1024)).hexdigest() for i,address in enumerate(mips)}}
            call['mips_aliased']=mips[0]==mips[1]
            report['samples'].append(call)
            if 'calibration' not in report:
                controls = dict(initial_controls,g_depth_active=True,g_perspective_active=True)
                if not args.reference_image:controls[control_key]=False
                restore(controls)
                ctrl = read_u32(DWT_CTRL)
                assert ctrl & 1, 'DWT counting is disabled; do not infer instruction work'
                before = read_u32(DWT_COUNT)
                assert read_u32(DWT_COUNT) == before, 'Counter advances while halted'
                count = 0
                while backend.read_register('pc') != (entry['lr'] & ~1):
                    backend.single_step()
                    count += 1
                    if count % 1000 == 0:
                        state = qmp_request(f'gwprov://{pid}', 'query-status')
                        assert state['return']['running'] is False
                    if count > 100000:
                        raise RuntimeError('Calibration call exceeded instruction bound')
                after = read_u32(DWT_COUNT)
                assert (after - before) & 0xffffffff == count, (before, after, count)
                report['calibration'] = dict(single_steps=count, dwt_before=before, dwt_after=after,
                                             dwt_ctrl=ctrl, output_hashes=result())
                print('DWT calibration', count, 'instructions exact', flush=True)
            def wrapper_call(target):
                backend.write_register('r1', target | 1)
                backend.write_register('r2', result_address)
                backend.write_register('pc', wrapper_address + wrapper_offset)
                run_to(dbg, entry['lr'])
                start, end = struct.unpack('<2I', backend.read_memory(result_address, 8))
                return start, end, (end - start) & 0xffffffff

            overheads = []
            for _ in range(3):
                restore(initial_controls)
                start, end, count = wrapper_call(wrapper_address + empty_offset)
                overheads.append(count - 1)  # Empty callee is exactly bx lr.
            assert len(set(overheads)) == 1, ('Wrapper overhead varies', overheads)
            call['wrapper_overhead_instructions'] = overheads[0]
            for perspective, depth in [(True, True), (False, True), (True, False), (False, False)]:
                expected = None
                counts = {}
                for enabled in ([False,False] if args.reference_image else [False, True, True, False]):
                    controls=dict(initial_controls,g_depth_active=depth,g_perspective_active=perspective)
                    if not args.reference_image:controls[control_key]=enabled
                    restore(controls)
                    assert read_u32(DWT_CTRL) & 1
                    before = read_u32(DWT_COUNT)
                    guest_start, guest_end, raw_instructions = wrapper_call(entry['pc'])
                    after = read_u32(DWT_COUNT)
                    instructions = raw_instructions - overheads[0]
                    assert 0 < instructions < 10000000
                    hashes = result()
                    if expected is None:
                        expected = hashes
                    assert hashes == expected, 'Specialized pixels/depth/counters differ'
                    assert not (backend.read_register('xpsr') & 0x1ff)
                    counts.setdefault(enabled, []).append(instructions)
                    call['measurements'].append(dict(perspective=perspective, depth=depth,
                                                      specialized=enabled if args.control=='opaque' and not args.reference_image else initial_controls.get('g_opaque_raster_enabled',False),
                                                      control_enabled=None if args.reference_image else enabled,
                                                      dwt_before=before, dwt_after=after,
                                                      guest_start=guest_start, guest_end=guest_end,
                                                      raw_in_guest_instructions=raw_instructions,
                                                      baseline_instructions=instructions, output_hashes=hashes))
                    print(label, perspective, depth, enabled, instructions, flush=True)
                    save_report()
                for values in counts.values():
                    assert len(set(values)) == 1, ('Repeated DWT counts unstable', counts)
                if label == 'small' and perspective and depth:
                    assert set(counts[False]) == {report['calibration']['single_steps']}, ('In-guest count differs from stepping', counts)
            save_report()
            # Complete this original call in the original scene before
            # collecting the next real entry. No measured changes carry over.
            restore(initial_controls)
            backend.write_memory(0xe000e100, enabled_irqs)
            backend.write_memory(0xe000e010, systick_ctrl.to_bytes(4, 'little'))
            if pending & (1 << 26):
                backend.write_memory(0xe000ed04, (1 << 26).to_bytes(4, 'little'))
            run_to(dbg, entry['lr'])
            if len(collected) == len(classes):
                break
        assert collected == set(classes), ('Missing real coverage classes', collected)
        report['faults'] = {hex(x): read_u32(x) for x in [0xe000ed28, 0xe000ed2c]}
        assert not any(report['faults'].values())
        report['measurement_valid'] = True
        save_report()
    except Exception as error:
        report['measurement_valid'] = False
        report['error'] = str(error) or type(error).__name__
        save_report()
        raise
    finally:
        if backend is not None:
            backend.close()
        if pid is not None:
            stopped = subprocess.run(['gwprov', 'device', f'gwemu:{pid}', 'stop'], capture_output=True, text=True)
            (args.output / 'cleanup.json').write_text(json.dumps(dict(pid=pid, stop_exit_code=stopped.returncode,
                stdout=stopped.stdout, stderr=stopped.stderr), indent=2) + '\n')
            if stopped.returncode:
                raise RuntimeError('Owned headless benchmark VM did not stop')


if __name__ == '__main__':
    main()
