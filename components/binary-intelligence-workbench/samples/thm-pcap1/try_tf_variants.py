import subprocess, hashlib, pathlib, binascii, os, itertools, string
ct='stream49_client.bin'
target='7181f4d45de00ae35b6cf8201c8d852b'
magic=[b'PK\x03\x04',b'\x89PNG',b'GIF8',b'\xff\xd8\xff',b'%PDF',b'HTB{',b'THM{',b'flag',b'FLAG',b'This',b'PNG',b'JFIF']

def keyhex(pw, keylen, mode):
    b=pw.encode()
    if mode=='zero': k=b[:keylen].ljust(keylen,b'\0')
    elif mode=='space': k=b[:keylen].ljust(keylen,b' ')
    elif mode=='repeat': k=(b*((keylen+len(b)-1)//len(b)))[:keylen]
    elif mode=='md5': k=hashlib.md5(b).digest(); k=(k*((keylen+15)//16))[:keylen]
    elif mode=='sha256': k=hashlib.sha256(b).digest()[:keylen]
    else: raise ValueError
    return k.hex()

cands=[l.rstrip('\n') for l in open('key_candidates.txt') if l.strip()]
for pw in cands:
  for keylen in (16,24,32):
    for kmode in ('zero','space','repeat','md5','sha256'):
      kh=keyhex(pw,keylen,kmode)
      for mode in ('ecbdec','cbcdec0','cfbdec0','cfbdecff'):
        out=f'/tmp/tf_dec_{os.getpid()}'
        r=subprocess.run(['./twofish_decrypt_blocks',kh,str(keylen),mode,ct,out],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
        if r.returncode: continue
        data=pathlib.Path(out).read_bytes()
        md5=hashlib.md5(data).hexdigest()
        score=0
        for m in magic:
            if m in data[:2000] or data.startswith(m): score+=10
        printable=sum(32<=x<127 or x in (9,10,13) for x in data[:512])
        if md5==target or score or printable>300:
            print('HIT?',pw,keylen,kmode,mode,'md5',md5,'print',printable,'head',data[:32].hex(),repr(data[:80]))
            if md5==target:
                pathlib.Path('decrypted_match.bin').write_bytes(data)
                raise SystemExit
print('done')
