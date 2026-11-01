from collections import defaultdict, deque, namedtuple

from assembly import header_figures, header_list, inside, merge_ranges, s32, u16, u32
from figures import FIGURE_POINTERS, PACKED_KEYS, TEXTURE_BYTES
from stages import GRAVITY_BYTES, STAGE_POINTERS, WIRE_GRAVITY

MIP_BYTES = 1024
PACKED_FAR_TEXTURES = 8
FLAT_FAR_TEXTURE = 0x80000000
DETAILED_FAR_TEXTURE = 0x40000000


def far_texture_payload(record):
    if record & FLAT_FAR_TEXTURE:
        return (record & 0x3FFFFF00) >> 6 if record & DETAILED_FAR_TEXTURE else None
    return record
Bank = namedtuple("Bank", "origin field at records")
FarBank = namedtuple("FarBank", "origin bank pairs table")


def compact_mip1(mip):
    rows = [mip[r * 64:r * 64 + 32] for r in range(16)]
    if mip != b"".join(row + bytes(32) for row in rows):
        raise ValueError("mip 1 must be 32x16 texels stored at a stride of 64")
    return b"".join(rows)


def index_of(table, data):
    return table.setdefault(data, len(table))


def overlap_units(units):
    count = len(units)
    following = {}
    preceding = {}
    chain = list(range(count))

    def chain_of(i):
        while chain[i] != i:
            chain[i] = chain[chain[i]]
            i = chain[i]
        return i

    for size in range(MIP_BYTES - 4, 0, -4):
        starts = {}
        for i in range(count):
            if i not in preceding:
                starts.setdefault(units[i][:size], deque()).append(i)
        for x in range(count):
            if x in following:
                continue
            bucket = starts.get(units[x][-size:])
            if not bucket:
                continue
            while bucket and bucket[0] in preceding:
                bucket.popleft()
            y = next((y for y in bucket if y not in preceding and chain_of(x) != chain_of(y)), None)
            if y is None:
                continue
            following[x] = (y, size)
            preceding[y] = x
            chain[chain_of(y)] = chain_of(x)

    pool = bytearray()
    offsets = {}
    for i in range(count):
        if i in preceding:
            continue
        offsets[i] = len(pool)
        pool += units[i]
        while i in following:
            i, size = following[i]
            offsets[i] = len(pool) - size
            pool += units[i][size:]
    return bytes(pool), offsets


class TexturePool:
    def __init__(self, assembly, header):
        self.memory = assembly.data
        self.symbols = assembly.symbols
        self.banks = []
        self.far_banks = []
        self.references = {}
        self.replaced = set()
        for name in header_list(header, "STAGE_BLOBS"):
            self.add_stage(name, far=False)
        for name in header_list(header, "STAGE_FAR_BLOBS"):
            if name != "NULL":
                self.add_stage(name, far=True)
        for name in header_figures(header):
            self.add_figure(name)

    def reference(self, at, origin, tag=0):
        self.references[at] = (origin + s32(self.memory, at) - tag, origin, tag)

    def add_bank(self, origin, field, count):
        at = origin + u32(self.memory, origin + field)
        records = [self.memory[at + i * TEXTURE_BYTES:at + (i + 1) * TEXTURE_BYTES] for i in range(count)]
        self.banks.append(Bank(origin, field, at, records))
        self.replaced.add(origin + field)

    def add_far_bank(self, origin, count):
        bank = origin + u32(self.memory, origin + STAGE_POINTERS["textures"])
        packed = u16(self.memory, origin + 186) & PACKED_FAR_TEXTURES
        records = [u32(self.memory, bank + i * 4) if packed else bank - origin + i * TEXTURE_BYTES
                   for i in range(count)]
        offsets = [far_texture_payload(record) for record in records]
        pairs = [None if offset is None else (origin + offset, origin + offset + MIP_BYTES)
                 for offset in offsets]
        for pair in pairs:
            if pair is not None:
                at = pair[0]
                self.banks.append(Bank(origin, None, at, [self.memory[at:at + TEXTURE_BYTES]]))
        self.far_banks.append(FarBank(origin, bank, pairs, f"texture_far_refs_{len(self.far_banks)}"))
        self.replaced.add(origin + STAGE_POINTERS["textures"])

    def add_stage(self, name, far):
        origin = self.symbols[name]
        count = u32(self.memory, origin + 8)
        if count and far:
            self.add_far_bank(origin, count)
        elif count:
            self.add_bank(origin, STAGE_POINTERS["textures"], count)
        for offset in STAGE_POINTERS.values():
            if u32(self.memory, origin + offset) and origin + offset not in self.replaced:
                self.reference(origin + offset, origin)
        for field in ("gravity_grid", "area_grid"):
            grid = u32(self.memory, origin + STAGE_POINTERS[field])
            for offset in (20, 24) if grid else ():
                if u32(self.memory, origin + grid + offset):
                    self.reference(origin + grid + offset, origin)
        gravities = origin + u32(self.memory, origin + STAGE_POINTERS["gravities"])
        for i in range(u32(self.memory, origin + 72)):
            entry = gravities + i * GRAVITY_BYTES
            if self.memory[entry] == WIRE_GRAVITY and u32(self.memory, entry + 44):
                self.reference(entry + 44, origin)

    def add_figure(self, name):
        origin = self.symbols[name]
        for field, count_field in (("textures", 4), ("lod_textures", 38)):
            count = u16(self.memory, origin + count_field)
            if count:
                self.add_bank(origin, FIGURE_POINTERS[field], count)
        for offset in FIGURE_POINTERS.values():
            if u32(self.memory, origin + offset) and origin + offset not in self.replaced:
                self.reference(origin + offset, origin)
        keys = u16(self.memory, origin + 6)
        if keys & PACKED_KEYS:
            table = origin + u32(self.memory, origin + FIGURE_POINTERS["keyframes"])
            for k in range(keys & 0x7FFF):
                self.reference(table + 4 * k, origin, 1)
                descriptor = origin + (u32(self.memory, table + 4 * k) & ~1)
                self.reference(descriptor + 4, descriptor)


def build_mip_pool(banks):
    mip0 = {}
    mip1 = {}
    ids = [[(index_of(mip0, record[:MIP_BYTES]), index_of(mip1, compact_mip1(record[MIP_BYTES:])))
            for record in bank.records] for bank in banks]
    units = list(mip0)
    mip1 = list(mip1) + [bytes(512)] * (len(mip1) % 2)
    for x, y in zip(mip1[::2], mip1[1::2]):
        units.append(b"".join(x[r * 32:r * 32 + 32] + y[r * 32:r * 32 + 32] for r in range(16)))
    pool, unit_at = overlap_units(units)
    tables = [tuple((unit_at[i], unit_at[len(mip0) + j // 2] + 32 * (j % 2)) for i, j in bank_ids)
              for bank_ids in ids]
    return pool + bytes(32), tables


def retained_ranges(removed, end):
    kept = []
    cursor = 0
    for low, high in removed + [(end, end)]:
        if cursor < low:
            kept.append((cursor, low))
        cursor = high
    return kept


def pool_textures(assets):
    assembly = assets.assemble()
    pool = TexturePool(assembly, assets.header)
    memory = assembly.data
    removed = merge_ranges([(bank.at, bank.at + len(bank.records) * TEXTURE_BYTES) for bank in pool.banks])
    names_at = defaultdict(list)
    for name, at in assembly.symbols.items():
        if not inside(at, removed):
            names_at[at].append(name)
    new_labels = {}

    def label_at(at):
        return new_labels.setdefault(at, f"texture_retained_{at}")

    words = dict(assembly.words)
    for at, (target, origin, tag) in pool.references.items():
        words[at] = f"{label_at(target)} - {label_at(origin)}" + (f" + {tag}" if tag else "")
    pool_bytes, bank_tables = build_mip_pool(pool.banks)
    tables = {}
    far_offsets = {}
    for bank, pairs in zip(pool.banks, bank_tables):
        if bank.field is None:
            far_offsets[bank.at] = pairs[0]
            continue
        label = tables.setdefault(pairs, f"texture_refs_{len(tables)}")
        words[bank.origin + bank.field] = f"{label} - {label_at(bank.origin)}"
    far_lines = []
    for bank in pool.far_banks:
        words[bank.origin + STAGE_POINTERS["textures"]] = f"{bank.table} - {label_at(bank.origin)}"
        far_lines += [f'    .section .rodata.assets.{bank.table}, "a"', "    .balign 4",
                      f"    .word {label_at(bank.bank)}", "    .word 0", f"    .global {bank.table}",
                      f"{bank.table}:"]
        for index, pair in enumerate(bank.pairs):
            if pair is not None:
                first, second = far_offsets[pair[0]]
                targets = [f"texture_mip_raw_pool + {first}", f"texture_mip_raw_pool + {second}"]
                if not u32(memory, bank.bank + index * 4) & FLAT_FAR_TEXTURE:
                    # Legacy raw records are still read as header-relative mip0
                    # by older runtimes when computing their average colour.
                    words[bank.bank + index * 4] = f"texture_mip_raw_pool + {first} - {label_at(bank.origin)}"
            else:
                targets = ["0", "0"]
            far_lines += [f"    .word {target}" for target in targets]

    section = "texture_retained_section_0.bin"
    lines = ['    .section .rodata.assets, "a"', "    .balign 4"]
    kept = retained_ranges(removed, len(memory))
    emitted = 0
    for first, last in kept:
        marks = {first, last}
        marks.update(at for at in words if first <= at < last)
        marks.update(at for at in names_at if first <= at < last)
        marks.update(at for at in new_labels if first <= at < last)
        marks = sorted(marks)
        for at, stop in zip(marks, marks[1:]):
            for name in sorted(names_at.get(at, ())):
                lines += [f"    .global {name}", f"{name}:"]
            if at in new_labels:
                lines.append(new_labels[at] + ":")
            if at in words:
                lines.append(f"    .word {words[at]}")
                at += 4
                emitted += 4
            if at < stop:
                lines.append(f'    .incbin "{section}", {emitted}, {stop - at}')
                emitted += stop - at
    assets.set_blob(section, b"".join(memory[first:last] for first, last in kept))

    assets.set_blob("texture_mip_raw_pool.bin", pool_bytes)
    lines += ['    .section .rodata.assets.raw_texture_pool, "a"', "    .balign 4",
              "    .global texture_mip_raw_pool", "texture_mip_raw_pool:",
              '    .incbin "texture_mip_raw_pool.bin"']
    for pairs, label in tables.items():
        lines += ["    .balign 4", f"    .global {label}", f"{label}:"]
        lines += [f"    .word texture_mip_raw_pool + {at}" for pair in pairs for at in pair]
    assets.text = "\n".join(lines + far_lines) + "\n"
    assets.header = assets.header.replace("#define ASSETS_GEN_H",
                                          "#define ASSETS_GEN_H\n#define GALAXY_RAW_TEXTURE_REFERENCES 2", 1)
