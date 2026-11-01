#!/usr/bin/env python3
"""Compare captured-row span kernels on the Docker host CPU, not the M7.

Uses process CPU time, balanced run order and a live checksum. This is a
host-side diagnostic of span setup only: no pixel stores, shader/depth work,
interrupts, cache placement or hardware-FPS inference. Quality is checked by
separate independent-oracle tests.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import statistics
import subprocess
import tempfile
from audit_perspective_spans_native import rows

ROOT = Path(__file__).resolve().parents[1]
PREFIX = '#include <stdint.h>\n#include <stdbool.h>\n#include <stdlib.h>\n#include <stdio.h>\n#include <time.h>\n'
MAIN = r'''
typedef struct {double v[6];uint32_t count;} Row;
int main(int argc,char**argv){
 if(argc!=3)return 2;FILE*f=fopen(argv[1],"rb");if(!f)abort();
 fseek(f,0,SEEK_END);long bytes=ftell(f);rewind(f);if(bytes<0||bytes%52)abort();
 unsigned n=bytes/52,repeats=strtoul(argv[2],NULL,10);Row*data=malloc(n*sizeof *data);if(!data)abort();
 for(unsigned i=0;i<n;i++)if(fread(data[i].v,48,1,f)!=1||fread(&data[i].count,4,1,f)!=1)abort();fclose(f);
 INIT_CONTROL
 struct timespec start,end;uint64_t checksum=0,spans=0;
 clock_gettime(CLOCK_PROCESS_CPUTIME_ID,&start);
 for(unsigned run=0;run<repeats;run++)for(unsigned i=0;i<n;i++){
  double*r=data[i].v;unsigned count=data[i].count,at=0;PerspectiveEndpoint endpoint={0};
  ROW_SETUP
  while(at<count){
   PerspectiveSpan s=raster_perspective_span(r[0]+at*r[3],r[1]+at*r[4],r[2]+at*r[5],r[3],r[4],r[5],count-at,&endpoint EXTRA_ARGUMENT);
   if(!s.count||s.count>count-at)abort();at+=s.count;spans++;
   // Consume every output, including UV increments used by the real pixel
   // loop, so optimization cannot discard their conversions or arithmetic.
   checksum+=(uint64_t)s.u+s.v+((uint64_t)s.du<<1)+((uint64_t)s.dv<<2)+s.count;
  }
 }
 clock_gettime(CLOCK_PROCESS_CPUTIME_ID,&end);free(data);
 double seconds=end.tv_sec-start.tv_sec+(end.tv_nsec-start.tv_nsec)*1e-9;
 printf("{\"cpu_seconds\":%.9f,\"checksum\":%llu,\"spans\":%llu}\n",seconds,(unsigned long long)checksum,(unsigned long long)spans);
}
'''


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('output', type=Path)
    p.add_argument('captures', type=Path, nargs='+')
    p.add_argument('--reference', type=Path, required=True)
    p.add_argument('--prior', type=Path, required=True)
    p.add_argument('--repeat', type=int, default=10000)
    a = p.parse_args()
    if a.repeat < 1:p.error('--repeat must be positive')
    sources = {'reference': a.reference, 'prior': a.prior, 'row': ROOT/'src/graphics/engine.c'}
    orders = [['reference','prior','row'], ['prior','row','reference'], ['row','reference','prior']]
    report = {'note':__doc__.strip(), 'repeat':a.repeat, 'orders':orders,
              'host_machine':platform.machine(),
              'sources':{k:{'path':str(v),'sha256':hashlib.sha256(v.read_bytes()).hexdigest()} for k,v in sources.items()},
              'captures':[]}
    with tempfile.TemporaryDirectory(prefix='span-bench-', dir=ROOT/'tmp') as directory:
        work = Path(directory)
        for i,capture in enumerate(a.captures):
            (work/f'{i}.bin').write_bytes(b''.join(rows(capture)))
        for variant,source_path in sources.items():
            s=source_path.read_text();helper=s[s.index('/* Perspective attributes'):s.index('static bool raster_span_bound(')]
            row_based='const double* curvature' in helper
            main=MAIN.replace('INIT_CONTROL','g_perspective_precheck_enabled=true;' if 'g_perspective_precheck_enabled' in helper else '')
            main=main.replace('ROW_SETUP','double curvature=raster_perspective_curvature(r[0],r[1],r[2],r[3],r[4],r[5]);' if row_based else '')
            main=main.replace('EXTRA_ARGUMENT',',&curvature' if row_based else '')
            (work/f'{variant}.c').write_text(PREFIX+helper+main)
        cpu=min(os.sched_getaffinity(0));report['pinned_host_cpu']=cpu
        base=['docker','run','--rm','--name',work.name,'--cpuset-cpus',str(cpu),'--user','1000:1000','-v',f'{work}:/test','-w','/test','smg-runtime-tests']
        try:
            report['compiler']=subprocess.run(base+['cc','--version'],check=True,capture_output=True,text=True,timeout=30).stdout.splitlines()[0]
            command=' && '.join(f'cc -O3 -fno-fast-math -fno-strict-aliasing {k}.c -o {k}' for k in sources)
            subprocess.run(base+['sh','-c',command],check=True,timeout=60)
            for i,capture in enumerate(a.captures):
                results={k:[] for k in sources}
                for order in orders:
                    for variant in order:
                        r=subprocess.run(base+[f'./{variant}',f'{i}.bin',str(a.repeat)],check=True,capture_output=True,text=True,timeout=60)
                        results[variant].append(json.loads(r.stdout))
                for values in results.values():assert len({(x['checksum'],x['spans']) for x in values})==1
                report['captures'].append({'capture':str(capture),'runs':results,
                    'median_cpu_seconds':{k:statistics.median(x['cpu_seconds'] for x in v) for k,v in results.items()}})
        finally:
            subprocess.run(['docker','rm','-f',work.name],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
    a.output.write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(report,indent=2))


if __name__=='__main__':main()
