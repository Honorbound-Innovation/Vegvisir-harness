#include <stdio.h>
#include <string.h>
#include <stdlib.h>
#include <ctype.h>
#include "twofish2.h"
extern char* generateKey(char* s);
int main(int argc,char**argv){ if(argc<4) return 2; FILE*fi=fopen(argv[2],"rb"); if(!fi) return 1; int maxoff=atoi(argv[3]); unsigned char all[30000]; int len=fread(all,1,sizeof all,fi); for(int off=0; off<maxoff && off+32<=len; off++){ char out1[17],out2[16]; unsigned char ob[9000]; memset(out1,0,17); TwoFish dec(generateKey(argv[1]), true, NULL, NULL); dec.setSocket(-1); dec.resetCBC(); dec.setOutputBuffer(ob); dec.blockCrypt((char*)all+off,out1,16); dec.flush(); dec.setOutputBuffer(ob); dec.blockCrypt((char*)all+off+16,out2,16); int n=atoi(out1); int ok=1,hs=0,hx=0; if(n<1||n>8192) ok=0; for(int i=0;i<16;i++){unsigned char c=out1[i]; if(c==' ') hs=1; if(c=='x') hx=1; if(!(isdigit(c)||c==' '||c=='x'||c=='\0')) ok=0;} if(ok&&hs&&hx) printf("%s\toff=%d\tn=%d\t[%.*s]\n",argv[1],off,n,16,out1); } return 0; }
