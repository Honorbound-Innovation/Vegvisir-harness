#!/usr/bin/env python3
import argparse, itertools, pathlib, re, socket, subprocess, sys, time
SBOX=[0x63,0x7c,0x77,0x7b,0xf2,0x6b,0x6f,0xc5,0x30,0x01,0x67,0x2b,0xfe,0xd7,0xab,0x76,0xca,0x82,0xc9,0x7d,0xfa,0x59,0x47,0xf0,0xad,0xd4,0xa2,0xaf,0x9c,0xa4,0x72,0xc0,0xb7,0xfd,0x93,0x26,0x36,0x3f,0xf7,0xcc,0x34,0xa5,0xe5,0xf1,0x71,0xd8,0x31,0x15,0x04,0xc7,0x23,0xc3,0x18,0x96,0x05,0x9a,0x07,0x12,0x80,0xe2,0xeb,0x27,0xb2,0x75,0x09,0x83,0x2c,0x1a,0x1b,0x6e,0x5a,0xa0,0x52,0x3b,0xd6,0xb3,0x29,0xe3,0x2f,0x84,0x53,0xd1,0x00,0xed,0x20,0xfc,0xb1,0x5b,0x6a,0xcb,0xbe,0x39,0x4a,0x4c,0x58,0xcf,0xd0,0xef,0xaa,0xfb,0x43,0x4d,0x33,0x85,0x45,0xf9,0x02,0x7f,0x50,0x3c,0x9f,0xa8,0x51,0xa3,0x40,0x8f,0x92,0x9d,0x38,0xf5,0xbc,0xb6,0xda,0x21,0x10,0xff,0xf3,0xd2,0xcd,0x0c,0x13,0xec,0x5f,0x97,0x44,0x17,0xc4,0xa7,0x7e,0x3d,0x64,0x5d,0x19,0x73,0x60,0x81,0x4f,0xdc,0x22,0x2a,0x90,0x88,0x46,0xee,0xb8,0x14,0xde,0x5e,0x0b,0xdb,0xe0,0x32,0x3a,0x0a,0x49,0x06,0x24,0x5c,0xc2,0xd3,0xac,0x62,0x91,0x95,0xe4,0x79,0xe7,0xc8,0x37,0x6d,0x8d,0xd5,0x4e,0xa9,0x6c,0x56,0xf4,0xea,0x65,0x7a,0xae,0x08,0xba,0x78,0x25,0x2e,0x1c,0xa6,0xb4,0xc6,0xe8,0xdd,0x74,0x1f,0x4b,0xbd,0x8b,0x8a,0x70,0x3e,0xb5,0x66,0x48,0x03,0xf6,0x0e,0x61,0x35,0x57,0xb9,0x86,0xc1,0x1d,0x9e,0xe1,0xf8,0x98,0x11,0x69,0xd9,0x8e,0x94,0x9b,0x1e,0x87,0xe9,0xce,0x55,0x28,0xdf,0x8c,0xa1,0x89,0x0d,0xbf,0xe6,0x42,0x68,0x41,0x99,0x2d,0x0f,0xb0,0x54,0xbb,0x16]
INV=[0]*256
for i,x in enumerate(SBOX): INV[x]=i
RCON=[0,1,2,4,8,0x10,0x20,0x40,0x80,0x1b,0x36]
def xtime(a): return ((a<<1)&0xff) ^ (0x1b if a&0x80 else 0)
def gf_mul(a,b):
    r=0
    while b:
        if b&1: r^=a
        a=xtime(a); b>>=1
    return r
# state memory is tiny-AES state[column][row] => index=col*4+row
GROUPS=[tuple(((col-row)%4)*4+row for row in range(4)) for col in range(4)]
PATTERNS=[]
for q in range(4):
    base=[0,0,0,0]; base[q]=1
    PATTERNS.append([gf_mul(2,base[0])^gf_mul(3,base[1])^base[2]^base[3], base[0]^gf_mul(2,base[1])^gf_mul(3,base[2])^base[3], base[0]^base[1]^gf_mul(2,base[2])^gf_mul(3,base[3]), gf_mul(3,base[0])^base[1]^base[2]^gf_mul(2,base[3])])
def key_expand(key):
    rk=list(key)+[0]*(176-16)
    for i in range(4,44):
        t=rk[(i-1)*4:i*4]
        if i%4==0:
            t=t[1:]+t[:1]; t=[SBOX[x] for x in t]; t[0]^=RCON[i//4]
        for j in range(4): rk[i*4+j]=rk[(i-4)*4+j]^t[j]
    return bytes(rk)
def invert_last_round_key(k10):
    words=[None]*44
    for i in range(4): words[40+i]=list(k10[i*4:i*4+4])
    for i in range(43,3,-1):
        if i%4==0:
            t=words[i-1][1:]+words[i-1][:1]; t=[SBOX[x] for x in t]; t[0]^=RCON[i//4]
        else: t=words[i-1]
        words[i-4]=[words[i][j]^t[j] for j in range(4)]
    return bytes(sum(words[:4],[]))
def inv_cipher_block(ct,rk):
    state=list(ct)
    def add_round(r):
        nonlocal state
        off=16*r; state=[state[i]^rk[off+i] for i in range(16)]
    def inv_shift():
        nonlocal state
        s=state[:]
        for c in range(4):
            for r in range(4): s[c*4+r]=state[((c-r)%4)*4+r]
        state=s
    def inv_sub():
        nonlocal state
        state=[INV[x] for x in state]
    def inv_mix_col(a):
        return [gf_mul(a[0],14)^gf_mul(a[1],11)^gf_mul(a[2],13)^gf_mul(a[3],9), gf_mul(a[0],9)^gf_mul(a[1],14)^gf_mul(a[2],11)^gf_mul(a[3],13), gf_mul(a[0],13)^gf_mul(a[1],9)^gf_mul(a[2],14)^gf_mul(a[3],11), gf_mul(a[0],11)^gf_mul(a[1],13)^gf_mul(a[2],9)^gf_mul(a[3],14)]
    def inv_mix():
        nonlocal state
        s=state[:]
        for c in range(4):
            out=inv_mix_col([state[c*4+r] for r in range(4)])
            for r in range(4): s[c*4+r]=out[r]
        state=s
    add_round(10)
    for r in range(9,0,-1): inv_shift(); inv_sub(); add_round(r); inv_mix()
    inv_shift(); inv_sub(); add_round(0)
    return bytes(state)
POSS={}
def poss_keys(a,b,d):
    k=(a,b,d)
    if k not in POSS: POSS[k]=[x for x in range(256) if (INV[a^x]^INV[b^x])==d]
    return POSS[k]
def recover_group(pairs, positions, verbose=False):
    cand=None
    for c,cf in pairs:
        vals=[(c[p],cf[p]) for p in positions]
        local=[]
        for pat in PATTERNS:
            for f in range(1,256):
                lists=[poss_keys(a,b,gf_mul(coef,f)) for (a,b),coef in zip(vals,pat)]
                if all(lists): local.extend(itertools.product(*lists))
        s=set(local)
        cand=s if cand is None else cand&s
        if verbose: print('  ',positions,'local',len(s),'intersection',len(cand))
        if not cand: return set()
    return cand
def solve_from_pairs(pairs, flagct, verbose=False):
    buckets={tuple(sorted(g)):[] for g in GROUPS}
    order={tuple(sorted(g)):g for g in GROUPS}
    for c,cf in pairs:
        diff=tuple(i for i,(a,b) in enumerate(zip(c,cf)) if a!=b)
        key=tuple(sorted(diff))
        if key in buckets: buckets[key].append((c,cf))
    if verbose: print('[+] bucket sizes', {k:len(v) for k,v in buckets.items()})
    k10=[None]*16
    for key,pos in order.items():
        cands=set()
        for m in (4,6,8,12,20,40,len(buckets[key])):
            if len(buckets[key]) < m: continue
            cands=recover_group(buckets[key][:m], pos, verbose=verbose)
            if verbose: print('[+] group',pos,'m',m,'cands',len(cands))
            if len(cands)==1: break
        if len(cands)!=1: raise RuntimeError(f'could not recover group {pos}; candidates={len(cands)}')
        for p,b in zip(pos,next(iter(cands))): k10[p]=b
    k10=bytes(k10); key=invert_last_round_key(k10); rk=key_expand(key)
    if rk[160:176]!=k10: raise RuntimeError('bad key schedule inversion')
    pt=b''.join(inv_cipher_block(flagct[i:i+16],rk) for i in range(0,len(flagct),16))
    return key,k10,pt
def parse_output(out):
    cs=[bytes.fromhex(x) for x in re.findall(r'Correct encryption: ([0-9a-f]{32})',out)]
    fs=[bytes.fromhex(x) for x in re.findall(r'Faulty encryption:\s+([0-9a-f]{32})',out)]
    m=re.search(r'Flag encrypted: ([0-9a-f]+)',out)
    if not m: raise RuntimeError('encrypted flag not found')
    return list(zip(cs,fs)), bytes.fromhex(m.group(1))
def collect_local(path,n):
    p=subprocess.run([str(path)],input=('y\n'*n+'n\n').encode(),stdout=subprocess.PIPE,stderr=subprocess.PIPE,timeout=max(20,n//10),cwd=path.parent)
    return parse_output(p.stdout.decode('latin1','replace'))
def recv_until(sock, token, timeout=20):
    sock.settimeout(timeout); data=b''
    while token not in data:
        chunk=sock.recv(4096)
        if not chunk: break
        data+=chunk
    return data
def collect_remote(host,port,n):
    with socket.create_connection((host,port),timeout=10) as s:
        data=b''
        for _ in range(n):
            data += recv_until(s,b'Encrypt once?')
            s.sendall(b'y\n')
        data += recv_until(s,b'Encrypt once?')
        s.sendall(b'n\n')
        while True:
            try:
                chunk=s.recv(4096)
                if not chunk: break
                data += chunk
            except socket.timeout: break
    return parse_output(data.decode('latin1','replace'))
def main():
    ap=argparse.ArgumentParser(description='AES DFA solver for HTB Wonky AES')
    ap.add_argument('--local', help='path to local enc_fault')
    ap.add_argument('--remote', nargs=2, metavar=('HOST','PORT'), help='remote host/port')
    ap.add_argument('-n','--samples', type=int, default=200)
    ap.add_argument('-v','--verbose', action='store_true')
    args=ap.parse_args()
    if args.remote:
        pairs,flagct=collect_remote(args.remote[0],int(args.remote[1]),args.samples)
    else:
        target=pathlib.Path(args.local or 'crypto_wonky_aes/enc_fault').resolve()
        pairs,flagct=collect_local(target,args.samples)
    print(f'[+] collected {len(pairs)} pairs')
    print(f'[+] encrypted flag: {flagct.hex()}')
    key,k10,pt=solve_from_pairs(pairs,flagct,args.verbose)
    print(f'[+] AES key: {key.hex()}')
    print(f'[+] round10 key: {k10.hex()}')
    print(f'[+] plaintext bytes: {pt!r}')
    print(pt.rstrip(b'\x00').decode('utf-8','replace'))
if __name__=='__main__': main()
