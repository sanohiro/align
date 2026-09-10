#define _POSIX_C_SOURCE 200809L
#include <assert.h>
#include <dlfcn.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stddef.h>
#include <time.h>
typedef struct { const unsigned char *ptr; int64_t len; } Str;
typedef struct { const unsigned char *name; int64_t len; int32_t tag; int64_t offset; const void *sub; int64_t opt; } Field;
static double now(void) { struct timespec t; assert(clock_gettime(CLOCK_MONOTONIC,&t)==0); return t.tv_sec + t.tv_nsec*1e-9; }
static void *symbol(void *lib, const char *name) { void *p=dlsym(lib,name); if(!p){fprintf(stderr,"%s: %s\n",name,dlerror());exit(2);} return p; }
int main(int argc,char **argv) {
 assert(argc==2); void *lib=dlopen(argv[1],RTLD_NOW|RTLD_LOCAL); assert(lib);
 int32_t (*decode)(const void*,int64_t,int32_t,void*)=symbol(lib,"align_rt_json_decode_scalar");
 void (*free_buf)(const void*)=symbol(lib,"align_rt_free");
 Str (*clone)(const void*,int64_t)=symbol(lib,"align_rt_str_clone");
 void *(*new_init)(void*,int32_t,int64_t)=dlsym(lib,"align_rt_json_builder_init");
 int32_t (*new_finish)(void*,Str*)=new_init?symbol(lib,"align_rt_json_builder_finish"):NULL;
 void *(*old_init)(void*,void*,int64_t)=symbol(lib,"align_rt_builder_init_stack");
 Str (*old_finish)(void*)=symbol(lib,"align_rt_builder_into_string_stack");
 void (*encode)(void*,const void*,const Field*,int64_t)=symbol(lib,"align_rt_json_encode_object");
 const char *tokens[]={"0.3","2","-0","1e-30","1e30","1.1754944e-38"};
 for(int width=4;width<=8;width+=4) {
   uint64_t out=0,checksum=0; size_t lengths[6];for(int j=0;j<6;j++)lengths[j]=strlen(tokens[j]);
   for(int i=0;i<6000;i++)assert(decode(tokens[i%6],lengths[i%6],512|width,&out)==0);
   double start=now();for(int i=0;i<1800000;i++){assert(decode(tokens[i%6],lengths[i%6],512|width,&out)==0);checksum^=out;}
   printf("decode_f%d %.3f %llu\n",width*8,(now()-start)*1e9/1800000,(unsigned long long)checksum);
 }
 const char *integer="18446744073709551615";uint64_t result=0;
 double start=now();for(int i=0;i<1800000;i++)assert(decode(integer,20,8,&result)==0);
 printf("decode_u64 %.3f %llu\n",(now()-start)*1e9/1800000,(unsigned long long)result);
 struct Row {double large;float small;int32_t pad;Str text;Str values;uint64_t present;double optional;} row;
 float values[]={0.3f,2.0f,-0.0f,1e-30f,1e30f,1.1754944e-38f};
 row=(struct Row){.large=0.3,.small=0.3f,.text={(void*)"text with quotes \" and UTF-8 日本語",37},.values={(void*)values,6},.present=1,.optional=-0.0};
 row.text.len=strlen((char*)row.text.ptr);
 Field fields[]={ {(void*)"large",5,520,offsetof(struct Row,large),NULL,-1}, {(void*)"small",5,516,offsetof(struct Row,small),NULL,-1}, {(void*)"text",4,2064,offsetof(struct Row,text),NULL,-1}, {(void*)"values",6,(7<<8)|16|(2<<20)|(4<<24),offsetof(struct Row,values),NULL,-1}, {(void*)"optional",8,520,offsetof(struct Row,optional),NULL,offsetof(struct Row,present)} };
 for(int copy=0;copy<2;copy++) {
   if(new_init && copy)continue;
   uint64_t total=0;double start=now();
   for(int i=0;i<200000;i++) { _Alignas(16) unsigned char header[64]; Str out;
     void *b=new_init?new_init(header,0,0):old_init(header,NULL,0);assert(b);encode(b,&row,fields,5);
     if(new_init)assert(new_finish(b,&out)==0);else out=old_finish(b);
     if(copy){Str owned=clone(out.ptr,out.len);free_buf(out.ptr);out=owned;}
     total+=out.len;if(i==0){printf("encoded ");fwrite(out.ptr,1,out.len,stdout);putchar('\n');}
     free_buf(out.ptr);
   }
   printf("encode_%s %.3f %llu\n",copy?"owned_copy":"raw",(now()-start)*1e9/200000,(unsigned long long)total);
 }
 dlclose(lib);
}
