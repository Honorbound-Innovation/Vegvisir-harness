#include <stdio.h>
#include <stdint.h>
#include <string.h>
#include <nettle/twofish.h>
int main(){
  struct twofish_ctx ctx; unsigned char key[16]={0}; unsigned char in[16]={0}; unsigned char out[16];
  twofish_set_key(&ctx,16,key); twofish_encrypt(&ctx,16,out,in);
  for(int i=0;i<16;i++) printf("%02x", out[i]); printf("\n");
  return 0;
}
