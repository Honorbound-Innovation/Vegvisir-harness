#include <stdio.h>
#include <string.h>
#include <stdlib.h>
#include <ctype.h>
#include "twofish2.h"
extern char* generateKey(char* s);
static void make_keybuf(const char*in, char*out, int mode){
  memset(out,0,128);
  if(mode==0) strcpy(out,in);                 // normal C string
  else { memcpy(out,in,32); out[32]=0; }      // cryptcat -k first 32 bytes, force terminator for safety after used range
}
int main(int argc,char**argv){ if(argc<4){fprintf(stderr,"usage key infile outfile [mode]\n");return 2;} int kmode=argc>4?atoi(argv[4]):0; char keybuf[128]; make_keybuf(argv[1],keybuf,kmode); FILE*fi=fopen(argv[2],"rb"),*fo=fopen(argv[3],"wb"); if(!fi||!fo) return 1; unsigned char all[30000]; int len=fread(all,1,sizeof all,fi); int pos=0, frames=0,totalout=0; while(pos+32<=len){ char out1[17],out2[16]; unsigned char ob[9000]; memset(out1,0,17); memset(ob,0,sizeof ob); TwoFish dec(generateKey(keybuf), true, NULL, NULL); dec.setSocket(-1); dec.resetCBC(); dec.setOutputBuffer(ob); dec.blockCrypt((char*)all+pos,out1,16); dec.flush(); dec.setOutputBuffer(ob); dec.blockCrypt((char*)all+pos+16,out2,16); int limit=atoi(out1); int valid=1; int hs=0,hx=0; if(limit<1||limit>8192||pos+32+limit>len) valid=0; for(int i=0;i<16;i++){unsigned char c=out1[i]; if(c==' ') hs=1; if(c=='x') hx=1; if(!(isdigit(c)||c==' '||c=='x'||c=='\0')) valid=0;} if(!valid||!hs||!hx){fprintf(stderr,"invalid at %d key[%s] mode%d header [",pos,argv[1],kmode); for(int i=0;i<16;i++){unsigned char c=out1[i]; if(c>=32&&c<127) fputc(c,stderr); else fprintf(stderr,"\\x%02x",c);} fprintf(stderr,"] atoi=%d\n",limit); return 3;} fprintf(stderr,"frame %d pos %d limit %d header [%.*s]\n",frames,pos,limit,16,out1); int loc=0,total=limit; dec.setOutputBuffer(ob); while(total>0){ char tmp[16]; int amount=total<16?total:16; dec.blockCrypt((char*)all+pos+32+loc,tmp,amount); total-=amount; loc+=16; } dec.flush(); fwrite(ob+32,1,limit,fo); totalout+=limit; pos += 32 + ((limit+15)/16)*16; frames++; } fprintf(stderr,"frames %d totalout %d consumed %d/%d\n",frames,totalout,pos,len); return 0; }
