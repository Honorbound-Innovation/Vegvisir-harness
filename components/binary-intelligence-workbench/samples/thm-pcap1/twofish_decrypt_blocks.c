#include <stdio.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>
#include <nettle/twofish.h>
static int hexval(int c){ if(c>='0'&&c<='9') return c-'0'; if(c>='a'&&c<='f') return c-'a'+10; if(c>='A'&&c<='F') return c-'A'+10; return -1; }
int main(int argc,char**argv){
 if(argc<6){fprintf(stderr,"usage: %s keyhex keylen mode infile outfile [skip]\n",argv[0]);return 2;}
 char*kh=argv[1]; int keylen=atoi(argv[2]); char*mode=argv[3]; int skip=argc>6?atoi(argv[6]):0;
 FILE*fi=fopen(argv[4],"rb"),*fo=fopen(argv[5],"wb"); if(!fi||!fo){perror("file");return 1;} if(skip) fseek(fi,skip,SEEK_SET);
 unsigned char key[32]; memset(key,0,sizeof key); for(int i=0;i<keylen;i++){int a=hexval(kh[2*i]),b=hexval(kh[2*i+1]); if(a<0||b<0){fprintf(stderr,"bad hex\n");return 1;} key[i]=(a<<4)|b;}
 struct twofish_ctx ctx; twofish_set_key(&ctx,keylen,key);
 unsigned char in[16],out[16],iv[16],tmp[16]; memset(iv,0,sizeof iv);
 if(strstr(mode,"ff")) memset(iv,0xff,16); if(strstr(mode,"aa")) memset(iv,'A',16); if(strstr(mode,"00str")) memcpy(iv,"0000000000000000",16);
 size_t n; while((n=fread(in,1,16,fi))>0){ if(n<16) memset(in+n,0,16-n);
   if(strncmp(mode,"ecbdec",6)==0) twofish_decrypt(&ctx,16,out,in);
   else if(strncmp(mode,"ecbenc",6)==0) twofish_encrypt(&ctx,16,out,in);
   else if(strncmp(mode,"cbcdec",6)==0){ twofish_decrypt(&ctx,16,tmp,in); for(int i=0;i<16;i++) out[i]=tmp[i]^iv[i]; memcpy(iv,in,16); }
   else if(strncmp(mode,"cfbdec",6)==0){ twofish_encrypt(&ctx,16,tmp,iv); for(int i=0;i<16;i++) out[i]=in[i]^tmp[i]; memcpy(iv,in,16); }
   else if(strncmp(mode,"ofbdec",6)==0){ twofish_encrypt(&ctx,16,iv,iv); for(int i=0;i<16;i++) out[i]=in[i]^iv[i]; }
   else {fprintf(stderr,"bad mode %s\n",mode);return 1;}
   fwrite(out,1,n,fo);
 }
 return 0;
}
