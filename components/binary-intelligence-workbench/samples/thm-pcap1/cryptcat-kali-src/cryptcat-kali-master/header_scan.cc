#include <stdio.h>
#include <string.h>
#include <stdlib.h>
#include <ctype.h>
#include "twofish2.h"
extern char* generateKey(char* s);
int main(int argc,char**argv){ if(argc<3) return 2; FILE*fi=fopen(argv[2],"rb"); if(!fi) return 1; char hdr[32]; if(fread(hdr,1,32,fi)!=32) return 1; char out1[17],out2[16]; unsigned char ob[9000]; memset(out1,0,17); TwoFish dec(generateKey(argv[1]), true, NULL, NULL); dec.setSocket(-1); dec.resetCBC(); dec.setOutputBuffer(ob); dec.blockCrypt(hdr,out1,16); dec.flush(); dec.setOutputBuffer(ob); dec.blockCrypt(hdr+16,out2,16); int ok=1; int n=atoi(out1); if(n<1 || n>8192) ok=0; int has_space=0, has_x=0; for(int i=0;i<16;i++){ unsigned char c=out1[i]; if(c==' ') has_space=1; if(c=='x') has_x=1; if(!(isdigit(c)||c==' '||c=='x'||c=='\0')) ok=0; } if(ok && has_space && has_x){ printf("%s\t%d\t",argv[1],n); for(int i=0;i<16;i++){unsigned char c=out1[i]; if(c>=32&&c<127) putchar(c); else printf("\\x%02x",c);} putchar('\n'); return 0;} return 3; }
