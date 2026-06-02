#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <sys/socket.h>
#include <sys/types.h>
#include <errno.h>
extern "C" void farm9crypt_init(char* keystr);
extern "C" int farm9crypt_read(int sockfd, char* buf, int size);
int main(int argc,char**argv){
  if(argc<4){fprintf(stderr,"usage: %s key in out\n",argv[0]); return 2;}
  FILE*f=fopen(argv[2],"rb"); if(!f){perror("in"); return 2;}
  unsigned char in[200000]; int n=fread(in,1,sizeof(in),f); fclose(f);
  int sv[2]; if(socketpair(AF_UNIX,SOCK_STREAM,0,sv)<0){perror("socketpair"); return 2;}
  int w=write(sv[0],in,n); if(w!=n){perror("write"); fprintf(stderr,"w=%d n=%d\n",w,n);}
  shutdown(sv[0],SHUT_WR);
  char keybuf[512]; memset(keybuf,0,sizeof keybuf); strcpy(keybuf,argv[1]);
  farm9crypt_init(keybuf);
  FILE*out=fopen(argv[3],"wb"); if(!out){perror("out"); return 2;}
  char buf[9000]; int total=0, r, iter=0;
  while((r=farm9crypt_read(sv[1],buf,8192))>0){
    fwrite(buf,1,r,out); total+=r; iter++; fprintf(stderr,"read %d total %d\n",r,total);
    if(iter>100) break;
  }
  fprintf(stderr,"final r=%d errno=%d total=%d\n",r,errno,total);
  fclose(out);
  return 0;
}
