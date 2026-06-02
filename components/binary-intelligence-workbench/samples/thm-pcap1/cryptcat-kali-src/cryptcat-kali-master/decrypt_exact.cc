#include <stdio.h>
#include <string.h>
#include <stdlib.h>
#include <ctype.h>
#include "twofish2.h"
extern char* generateKey(char* s);
int decrypt_one(const char* key, const unsigned char* in, int len, unsigned char* plain){
  char keybuf[128]; memset(keybuf,0,sizeof keybuf); strcpy(keybuf,key);
  TwoFish decryptor(generateKey(keybuf), true, NULL, NULL); decryptor.setSocket(-1);
  int pos=0, outpos=0;
  while(pos+32<=len){
    char outbuf[16], outbuf2[16]; static char outBuffer[8193]; static char inBuffer[8193]; memset(outBuffer,0,sizeof outBuffer); memset(inBuffer,0,sizeof inBuffer);
    decryptor.resetCBC();
    decryptor.setOutputBuffer((unsigned char*)&outBuffer[0]);
    decryptor.blockCrypt((char*)in+pos, outbuf, 16);
    decryptor.flush();
    decryptor.setOutputBuffer((unsigned char*)&outBuffer[0]);
    decryptor.blockCrypt((char*)in+pos+16, outbuf2, 16);
    int limit=atoi(outbuf);
    int valid=1,hs=0,hx=0; if(limit<1||limit>8192||pos+32+limit>len) valid=0;
    for(int i=0;i<16;i++){unsigned char c=outbuf[i]; if(c==' ')hs=1; if(c=='x')hx=1; if(!(isdigit(c)||c==' '||c=='x'||c=='\0')) valid=0;}
    if(!valid||!hs||!hx) return -1;
    memcpy(inBuffer, in+pos+32, limit);
    int total=limit, loc=0; char tmp[16]; char* obuf=&outBuffer[0];
    while(total>0){ int amount=16; if(total<amount) amount=total; decryptor.blockCrypt(inBuffer+loc,tmp,amount); total-=amount; loc+=16; }
    decryptor.flush();
    memcpy(plain+outpos, obuf+32, limit); outpos += limit;
    pos += 32 + limit;
  }
  return outpos;
}
int main(int argc,char**argv){ if(argc<4) return 2; FILE*fi=fopen(argv[2],"rb"); FILE*fo=fopen(argv[3],"wb"); unsigned char in[30000],plain[30000]; int len=fread(in,1,sizeof in,fi); int n=decrypt_one(argv[1],in,len,plain); if(n<0){fprintf(stderr,"bad key %s\n",argv[1]); return 3;} fwrite(plain,1,n,fo); fprintf(stderr,"ok %s len %d\n",argv[1],n); return 0; }
