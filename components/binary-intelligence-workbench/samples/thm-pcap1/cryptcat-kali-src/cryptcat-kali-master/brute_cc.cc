#include <stdio.h>
#include <string.h>
#include <stdlib.h>
#include "twofish2.h"
extern char* generateKey(char* s);
static int dec_try(const char* k,const unsigned char* all,int len,int off,unsigned char*out){
 char keybuf[256]; memset(keybuf,0,sizeof keybuf); strcpy(keybuf,k);
 TwoFish dec(generateKey(keybuf), true, NULL, NULL); dec.setSocket(-1);
 int pos=off,outpos=0,frames=0;
 while(pos+32<=len && frames<100){
  char hdr[16], hdr2[16], tmp[16]; static char ob[9000], ib[9000]; memset(ob,0,sizeof ob); memset(ib,0,sizeof ib); memset(hdr,0,sizeof hdr);
  dec.resetCBC(); dec.setOutputBuffer((unsigned char*)ob); dec.blockCrypt((char*)all+pos,hdr,16); dec.flush(); dec.setOutputBuffer((unsigned char*)ob); dec.blockCrypt((char*)all+pos+16,hdr2,16);
  int limit=atoi(hdr);
  if(limit<=0||limit>8192||pos+32+limit>len) return -1;
  // require first header starts with ascii digit; subsequent too
  if(hdr[0]<'0'||hdr[0]>'9') return -1;
  memcpy(ib,all+pos+32,limit); int total=limit,loc=0;
  while(total>0){int amount=total<16?total:16; dec.blockCrypt(ib+loc,tmp,amount); total-=amount; loc+=16;}
  dec.flush(); memcpy(out+outpos,ob+32,limit); outpos+=limit; pos+=32+limit; frames++;
 }
 if(pos!=len) return -1; return outpos;
}
int main(int argc,char**argv){ if(argc<4) return 2; FILE*f=fopen(argv[2],"rb"); unsigned char in[100000],out[100000]; int n=fread(in,1,sizeof in,f); for(int off=0;off<atoi(argv[3]);off++){int m=dec_try(argv[1],in,n,off,out); if(m>0){printf("key=%s off=%d len=%d\n",argv[1],off,m); fwrite(out,1,m,stdout); return 0;}} return 3;}
