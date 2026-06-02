#include <stdio.h>
#include <string.h>
#include <stdlib.h>
#include <ctype.h>
#include "twofish2.h"
extern char* generateKey(char* s);
static int decrypt_all(const char* k, const unsigned char* all, int len, unsigned char* plain){
  char keybuf[256]; memset(keybuf,0,sizeof keybuf); strcpy(keybuf,k);
  TwoFish dec(generateKey(keybuf), true, NULL, NULL); dec.setSocket(-1);
  int pos=0,outpos=0,frame=0;
  while(pos+32<=len){
    char outbuf[16]; char outbuf2[16]; static char outBuffer[9000]; static char inBuffer[9000];
    memset(outBuffer,0,sizeof outBuffer); memset(inBuffer,0,sizeof inBuffer); memset(outbuf,0,sizeof outbuf); memset(outbuf2,0,sizeof outbuf2);
    dec.resetCBC(); dec.setOutputBuffer((unsigned char*)outBuffer);
    dec.blockCrypt((char*)all+pos,outbuf,16); dec.flush();
    dec.setOutputBuffer((unsigned char*)outBuffer);
    dec.blockCrypt((char*)all+pos+16,outbuf2,16);
    int limit=atoi(outbuf);
    fprintf(stderr,"frame %d pos %d rawhdr ",frame,pos);
    for(int i=0;i<16;i++){ unsigned char c=outbuf[i]; if(c>=32&&c<127) fputc(c,stderr); else fprintf(stderr,"\\x%02x",c); }
    fprintf(stderr," limit %d\n",limit);
    if(limit<=0 || limit>8192 || pos+32+limit>len) return -1;
    memcpy(inBuffer, all+pos+32, limit);
    int total=limit, loc=0; char tmp[16];
    while(total>0){ int amount=total<16?total:16; dec.blockCrypt(inBuffer+loc,tmp,amount); total-=amount; loc+=16; }
    dec.flush();
    memcpy(plain+outpos, outBuffer+32, limit);
    outpos+=limit; pos+=32+limit; frame++;
  }
  return outpos;
}
int main(int argc,char**argv){ if(argc<4){fprintf(stderr,"usage key in out\n"); return 2;} FILE*f=fopen(argv[2],"rb"); unsigned char in[100000],out[100000]; int n=fread(in,1,sizeof in,f); int m=decrypt_all(argv[1],in,n,out); if(m<0){fprintf(stderr,"bad\n"); return 3;} FILE*g=fopen(argv[3],"wb"); fwrite(out,1,m,g); fprintf(stderr,"ok %d\n",m); }
