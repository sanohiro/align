import ctypes as c,sys,json
lib=c.CDLL(sys.argv[1])
class Str(c.Structure):_fields_=[('ptr',c.c_void_p),('len',c.c_int64)]
class Field(c.Structure):_fields_=[('name',c.c_char_p),('len',c.c_int64),('tag',c.c_int32),('offset',c.c_int64),('sub',c.c_void_p),('opt',c.c_int64)]
class Row(c.Structure):_fields_=[('large',c.c_double),('small',c.c_float),('pad',c.c_int32),('text',Str),('values',Str),('present',c.c_uint64),('optional',c.c_double)]
def fn(name,args,ret):
 f=getattr(lib,name);f.argtypes=args;f.restype=ret;return f
free=fn('align_rt_free',[c.c_void_p],None)
clone=fn('align_rt_str_clone',[c.c_void_p,c.c_int64],Str)
reset=fn('align_rt_requested_live_reset',[],None)
live=fn('align_rt_requested_live_bytes',[],c.c_int64)
peak=fn('align_rt_requested_live_peak',[],c.c_int64)
encode=fn('align_rt_json_encode_object',[c.c_void_p,c.c_void_p,c.POINTER(Field),c.c_int64],None)
new=hasattr(lib,'align_rt_json_builder_init')
init=fn('align_rt_json_builder_init' if new else 'align_rt_builder_init_stack',[c.c_void_p,c.c_int32 if new else c.c_void_p,c.c_int64],c.c_void_p)
finish=fn('align_rt_json_builder_finish' if new else 'align_rt_builder_into_string_stack',[c.c_void_p,c.POINTER(Str)] if new else [c.c_void_p],c.c_int32 if new else Str)
text=c.create_string_buffer('text with quotes " and UTF-8 日本語'.encode())
values=(c.c_float*6)(0.3,2,-0.0,1e-30,1e30,1.1754944e-38)
row=Row(.3,.3,0,Str(c.addressof(text),len(text.value)),Str(c.addressof(values),6),1,-0.0)
fields=(Field*5)(*[Field(name.encode(),len(name),tag,getattr(Row,name).offset,None,opt) for name,tag,opt in [('large',520,-1),('small',516,-1),('text',2064,-1),('values',(7<<8)|16|(2<<20)|(4<<24),-1),('optional',520,Row.present.offset)]])
for copying in ([False] if new else [False,True]):
 header=(c.c_uint64*8)();assert c.addressof(header)%16==0
 reset();b=init(header,0 if new else None,0);assert b
 encode(b,c.byref(row),fields,5)
 if new:
  out=Str();assert finish(b,c.byref(out))==0
 else:out=finish(b)
 if copying:
  copied=clone(out.ptr,out.len);free(out.ptr);out=copied
 result=dict(mode='current' if new else 'baseline-copy' if copying else 'baseline-raw',length=out.len,live=live(),peak=peak())
 free(out.ptr);result['after_free']=live();assert result['after_free']==0
 print(json.dumps(result))
