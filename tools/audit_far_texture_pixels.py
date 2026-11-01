#!/usr/bin/env python3
"""Compare every linked far mip texel with its compiler input, including old-reader averages."""
from pathlib import Path
import argparse,json,struct
from elftools.elf.elffile import ELFFile
p=argparse.ArgumentParser(description=__doc__);p.add_argument('elf',type=Path);p.add_argument('assets',type=Path);args=p.parse_args()
with args.elf.open('rb') as f:
 e=ELFFile(f);section=e.get_section_by_name('.xip_smg_assets');base=section['sh_addr'];image=section.data();symbols=e.get_section_by_name('.symtab')
 def at(address,size):
  offset=address-base;assert 0<=offset<=len(image)-size;return image[offset:offset+size]
 rows=[]
 for path in sorted(args.assets.glob('stage_*_far.bin')):
  source=path.read_bytes();count=struct.unpack_from('<I',source,8)[0];bank=struct.unpack_from('<I',source,36)[0]
  origin=symbols.get_symbol_by_name(path.stem)[0]['st_value'];table=origin+struct.unpack('<I',at(origin+36,4))[0]
  records=struct.unpack('<I',at(table-8,4))[0];details=0
  for i in range(count):
   original=struct.unpack_from('<I',source,bank+4*i)[0];record=struct.unpack('<I',at(records+4*i,4))[0]
   if original&0x80000000:
    assert record==original
    if not original&0x40000000:continue
    offset=(original&0x3fffff00)>>6;details+=1
   else:offset=original
   expected=source[offset:offset+2048];assert len(expected)==2048
   mip0,mip1=struct.unpack('<2I',at(table+i*8,8));assert at(mip0,1024)==expected[:1024],(path.name,i,'mip0')
   for row in range(16):assert at(mip1+row*64,32)==expected[1024+row*64:1024+row*64+32],(path.name,i,'mip1',row)
   if not original&0x80000000:assert at(origin+record,1024)==expected[:1024],(path.name,i,'legacy average source')
  rows.append({'stage':path.stem,'materials':count,'new_detailed_materials':details})
print(json.dumps({'rows':rows,'new_detailed_materials':sum(r['new_detailed_materials'] for r in rows),'all_mip_pixels_exact':True,'legacy_average_records_and_raw_sources_preserved':True},indent=2))
