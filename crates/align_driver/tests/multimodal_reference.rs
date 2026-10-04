//! Plan 91: the runnable reference owns its queues, wire records and native release boundary.
mod common;
use common::*;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};
static OWNER_SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn sources() -> Vec<(String, String)> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/multimodal");
    [
        "main",
        "job_model",
        "controller",
        "execution",
        "observer",
        "transport",
        "worker",
    ]
    .into_iter()
    .map(|name| {
        (
            format!("{name}.align"),
            std::fs::read_to_string(root.join(format!("{name}.align"))).unwrap(),
        )
    })
    .collect()
}
fn refs(files: &[(String, String)]) -> Vec<(&str, &str)> {
    files
        .iter()
        .map(|(name, source)| (name.as_str(), source.as_str()))
        .collect()
}
struct ChildOwner(Child);
impl Drop for ChildOwner {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            // Interrupt Python first so its immediately armed finally closes streams,
            // requests scope cleanup and reaps the server before removing its private root.
            #[cfg(unix)]
            unsafe {
                libc::kill(
                    libc::pid_t::try_from(self.0.id()).expect("native pid range"),
                    libc::SIGINT,
                );
            }
            let cleanup_deadline = Instant::now() + Duration::from_secs(7);
            while Instant::now() < cleanup_deadline {
                if self.0.try_wait().ok().flatten().is_some() {
                    return;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}
fn run_bounded(
    mut command: Command,
    directory: &Path,
    label: &str,
    budget: Duration,
) -> (std::process::ExitStatus, String, String) {
    let out = directory.join(format!("{label}.stdout"));
    let err = directory.join(format!("{label}.stderr"));
    command.stdout(Stdio::from(std::fs::File::create(&out).unwrap()));
    command.stderr(Stdio::from(std::fs::File::create(&err).unwrap()));
    let deadline = Instant::now() + budget - Duration::from_secs(8);
    let mut child = ChildOwner(command.spawn().unwrap());
    let status = loop {
        if let Some(status) = child.0.try_wait().unwrap() {
            break status;
        }
        assert!(
            Instant::now() < deadline,
            "{label} exceeded its one launch/setup/run budget"
        );
        std::thread::sleep(Duration::from_millis(10));
    };
    let read = |path: PathBuf| {
        let mut bytes = Vec::new();
        std::fs::File::open(path)
            .unwrap()
            .take(1_048_577)
            .read_to_end(&mut bytes)
            .unwrap();
        assert!(bytes.len() <= 1_048_576, "owner output bound");
        String::from_utf8(bytes).unwrap()
    };
    (status, read(out), read(err))
}
#[test]
fn model_and_wire_goldens_agree_whole_and_per_unit() {
    if !backend_available() {
        return;
    }
    // Keep compiler CPU work out of the native application's finite I/O budgets.
    let _owner = OWNER_SERIAL.lock().unwrap();
    let mut files = sources();
    files[0].1 = "import controller\nimport job_model\nfn main() -> Result<(), Error> { controller.selftest()?; return job_model.selftest() }\n".to_owned();
    let refs = refs(&files);
    let checked = diff_check_multi("multimodal-model", &refs, "main.align");
    assert!(
        !checked.whole_errors && !checked.per_unit_errors,
        "{}\n{}",
        checked.whole_diags,
        checked.per_unit_diags
    );
    let whole = build_exe_multi("multimodal-model-whole", &refs, "main.align");
    let per_unit = build_per_unit_multi("multimodal-model-unit", &refs, "main.align");
    let objects = per_unit.emit_objects(false);
    let object_refs: Vec<&Path> = objects.iter().map(PathBuf::as_path).collect();
    let unit_exe = per_unit.dir.join("model-unit");
    link_objects(
        &align_driver::CDriver::default(),
        &object_refs,
        &unit_exe,
        &per_unit.link_libs_union(),
        Profile::Release,
    )
    .unwrap();
    let expected = concat!(
        "controller-model-ok\n",
        "{\"artifact_id\":\"opaque\",\"transcript\":\"€a\",\"sample_rate\":8000,\"channels\":1,\"sample_frames\":8000,\"producer\":\"fixture\",\"entries\":[{\"ordinal\":0,\"track\":0,\"kind\":\"word\",\"label\":\"€\",\"start_frame\":400,\"end_frame\":800,\"text_span\":{\"start\":0,\"end\":3}}]}\n",
        "{\"artifact_id\":\"opaque\",\"transcript\":\"\",\"sample_rate\":8000,\"channels\":1,\"sample_frames\":0,\"producer\":\"fixture\"}\n",
        "{\"epoch\":\"0123456789abcdef0123456789abcdef\",\"id\":1,\"state\":\"queued\",\"phase\":\"\",\"completed\":0,\"sequence\":1,\"oldest_sequence\":1,\"gap\":false,\"events\":[{\"sequence\":1,\"state\":\"queued\",\"phase\":\"\",\"completed\":0}],\"artifacts\":[]}\n",
        "model-ok\n"
    );
    for (label, executable) in [("whole", &whole.exe), ("unit", &unit_exe)] {
        let (status, output, error) = run_bounded(
            Command::new(executable),
            executable.parent().unwrap(),
            label,
            Duration::from_secs(10),
        );
        assert!(status.success(), "{error}");
        assert_eq!(output, expected);
    }
}
#[test]
fn linux_pipeline_control_and_lease_lifetimes() {
    if !backend_available() || !cfg!(target_os = "linux") {
        return;
    }
    if !Path::new("/usr/bin/ffmpeg").is_file() || !Path::new("/usr/bin/ffprobe").is_file() {
        eprintln!(
            "FFmpeg composition qualification requires explicit /usr/bin/ffmpeg and ffprobe; run the Linux owner with those tools installed"
        );
        return;
    }
    // Keep compiler CPU work out of the native application's finite I/O budgets.
    let _owner = OWNER_SERIAL.lock().unwrap();
    let mut files = sources();
    assert_eq!(files[0].1.matches("  token := nonce()").count(), 1);
    files[0].1 = files[0].1.replace(
        "  token := nonce()",
        "  token := \"fixture-private-control-token\".clone()",
    );
    let worker = files
        .iter_mut()
        .find(|(name, _)| name == "worker.align")
        .unwrap();
    let delay = "time.instant() - started >= delay_ns";
    assert_eq!(worker.1.matches(delay).count(), 1);
    // A controlled three-second producer keeps capacity/cancellation observable without a race.
    worker.1 = worker.1.replace(
        delay,
        "time.instant() - started >= if delay_ns == 0 { 0 } else { 3000000000 }",
    );
    let transport = files
        .iter_mut()
        .find(|(name, _)| name == "transport.align")
        .unwrap();
    let capture = "  stream := ctx.respond_stream(response)?";
    assert_eq!(transport.1.matches(capture).count(), 1);
    transport.1 = transport.1.replace(capture, "  stalled := (ctx.headers().get(\"X-Owner-Stall\") else { \"\" }) == \"yes\"\n  stream := ctx.respond_stream(response)?");
    let budget = "  stream.write_timeout_ns(50000000)?";
    assert_eq!(transport.1.matches(budget).count(), 1);
    // Exercise the actual control reply's budget with a socket-filling fixture payload.
    // Ordinary bounded status JSON can fit the kernel buffer even when the peer never reads.
    transport.1 = transport.1.replace(budget, "  stream.write_timeout_ns(50000000)?\n  if stalled {\n    payload := buffer.filled(4096, 120)\n    mut index := 0\n    loop { if index == 2048 { break }; stream.send(payload.bytes())?; index = index + 1 }\n  }");
    let post_header = "  request.header(\"X-Observer-Slot\", slot_value)";
    assert_eq!(transport.1.matches(post_header).count(), 1);
    transport.1 = transport.1.replace(post_header, "  request.header(\"X-Observer-Slot\", slot_value)\n  if slot == 1 && match path.find(\"/lease/\") { Some(_) => true, None => false } { request.header(\"X-Owner-Bad-Grant\", \"yes\") }");
    let controller = files
        .iter_mut()
        .find(|(name, _)| name == "controller.align")
        .unwrap();
    let encoded_grant = "    body := json.encode_bounded(grant, 32768)?";
    assert_eq!(controller.1.matches(encoded_grant).count(), 1);
    controller.1 = controller.1.replace(encoded_grant, "    body := json.encode_bounded(grant, 32768)?\n    if (ctx.headers().get(\"X-Owner-Bad-Grant\") else { \"\" }) == \"yes\" { return transport.reply(ctx, 200, \"application/json\", \"{}\".bytes()) }");
    let observer = files
        .iter_mut()
        .find(|(name, _)| name == "observer.align")
        .unwrap();
    let cursor = "  mut cursor := job_model.canonical_id";
    assert_eq!(observer.1.matches(cursor).count(), 1);
    observer.1 = observer.1.replace(cursor, "  forced_late := (ctx.headers().get(\"X-Owner-Late\") else { \"\" }) == \"yes\"\n  mut native_snapshot_count := 0\n  mut cursor := job_model.canonical_id");
    let fetched = "    snapshot_response := transport.private_get(client, control, token, path)?";
    assert_eq!(observer.1.matches(fetched).count(), 1);
    observer.1 = observer.1.replace(fetched, "    snapshot_response := transport.private_get(client, control, token, path)?\n    native_snapshot_count = native_snapshot_count + 1\n    snapshot_status := if forced_late && native_snapshot_count > 1 { 404 } else { snapshot_response.status() }");
    assert_eq!(
        observer
            .1
            .matches("snapshot_response.status() != 200")
            .count(),
        1
    );
    observer.1 = observer.1.replace(
        "snapshot_response.status() != 200",
        "snapshot_status != 200",
    );
    assert_eq!(
        observer
            .1
            .matches("http.response(snapshot_response.status())")
            .count(),
        1
    );
    observer.1 = observer.1.replace(
        "http.response(snapshot_response.status())",
        "http.response(snapshot_status)",
    );
    let built = build_per_unit_multi("multimodal-native", &refs(&files), "main.align");
    let objects = built.emit_objects(false);
    let object_refs: Vec<&Path> = objects.iter().map(PathBuf::as_path).collect();
    let executable = built.dir.join("native-unit");
    link_objects(
        &align_driver::CDriver::default(),
        &object_refs,
        &executable,
        &built.link_libs_union(),
        Profile::Release,
    )
    .unwrap();
    let mut command = Command::new("python3");
    command.arg("-c").arg(NATIVE_OWNER).arg(&executable);
    let (status, output, error) =
        run_bounded(command, &built.dir, "native", Duration::from_secs(90));
    assert!(status.success(), "stdout={output}\nstderr={error}");
    assert!(output.contains("reference-integration-ok"), "{output}");
}
const NATIVE_OWNER: &str = r###"
import ctypes, http.client, json, os, pathlib, shutil, signal, socket, subprocess, sys, tempfile, time, urllib.request, urllib.error
exe=sys.argv[1]
# This dedicated single-thread Python child owns every fixture process family.
libc=ctypes.CDLL(None,use_errno=True)
libc.prctl.argtypes=[ctypes.c_int,ctypes.c_ulong,ctypes.c_ulong,ctypes.c_ulong,ctypes.c_ulong]
libc.prctl.restype=ctypes.c_int
prior_subreaper=ctypes.c_int()
assert libc.prctl(37,ctypes.addressof(prior_subreaper),0,0,0)==0
prior_sigchld=signal.signal(signal.SIGCHLD,signal.SIG_DFL)
assert libc.prctl(36,1,0,0,0)==0

def reap_owned():
 end=time.monotonic()+4;total=0
 while True:
  # Unreaped direct children cannot have their PIDs recycled. No other reaper runs.
  with pathlib.Path(f'/proc/self/task/{os.getpid()}/children').open() as listing:raw=listing.read(65537)
  assert len(raw)<=65536,'fixture child-table byte bound'
  children=[int(value) for value in raw.split()]
  if not children:return total
  assert len(children)<=4096 and time.monotonic()<end,'fixture descendant cleanup bound'
  handles=[]
  try:
   for pid in children:
    descriptor=os.pidfd_open(pid);handles.append((pid,descriptor))
    try:signal.pidfd_send_signal(descriptor,signal.SIGKILL)
    except ProcessLookupError:pass
   for pid,descriptor in handles:
    while True:
     waited,status=os.waitpid(pid,os.WNOHANG)
     if waited==pid:total+=1;break
     assert time.monotonic()<end,'fixture descendant reap budget'
     time.sleep(.001)
  finally:
   for pid,descriptor in handles:os.close(descriptor)

sandbox=pathlib.Path(tempfile.mkdtemp(prefix='align-reference-native-'))
root=sandbox/'artifacts';root.mkdir(mode=0o700)
def port():
 s=socket.socket(); s.bind(('127.0.0.1',0)); p=s.getsockname()[1]; s.close(); return p
control, observation, observation_second=port(),port(),port()
assert len({control,observation,observation_second})==3
token="fixture-private-control-token"
log=sandbox/'server.log'
handle=log.open('wb')
server=None
deadline=time.monotonic()+60
native_reader=None
first=second=None
def alarm(sig, frame):raise TimeoutError('native fixture budget')
signal.signal(signal.SIGALRM,alarm)
signal.alarm(75)

def request(method,path,data=None,observer=False,private=False,slot=0,headers=None):
 payload=None if data is None else data if isinstance(data,bytes) else json.dumps(data).encode()
 req=urllib.request.Request(f'http://127.0.0.1:{[control,observation,observation_second][observer]}'+path,data=payload,method=method)
 for name,value in (headers or {}).items():req.add_header(name,value)
 if private:
  req.add_header("X-Reference-Control",token)
  req.add_header("X-Observer-Slot",str(slot))
 for attempt in range(3 if method=='GET' else 1):
  try:
   with urllib.request.urlopen(req,timeout=2) as response:return response.status,response.read()
  except urllib.error.HTTPError as error:return error.code,error.read()
  except (urllib.error.URLError,TimeoutError,ConnectionError):
   # A finite total acquisition budget may expire immediately after selection.
   # Retry only reads; an unacknowledged submit must never silently enqueue twice.
   if method!='GET' or attempt==2:raise

def get_json(path):
 code,body=request('GET',path); assert code==200,(code,body);return json.loads(body)
try:
 server=subprocess.Popen([exe,'serve',str(root),str(control),str(observation),str(observation_second),'/usr/bin/ffmpeg'],stderr=handle,start_new_session=True)
 while True:
  try:
   health=get_json('/health'); break
  except Exception:
   if server.poll() is not None or time.monotonic()>deadline:raise AssertionError(log.read_text())
   time.sleep(.02)
 code,body=request('POST','/v1/jobs/00000000000000000000000000000000/999');assert code==405
 code,body=request('GET',f"/v1/jobs/{health['epoch']}/999/cancel");assert code==405
 code,body=request('GET',f"/internal/jobs/{health['epoch']}/999/lease/audio",private=True);assert code==405
 code,body=request('GET','/internal/slots/0/release/1',private=True);assert code==405
 code,body=request('POST',f"/v1/jobs/{health['epoch']}/-1/events",observer=True);assert code==400
 code,body=request('GET','/internal/health');assert code==403
 code,body=request('POST','/v1/jobs/audio',b'{"input":"x","input":"y","delay_ns":0}');assert code==400
 code,body=request('POST','/v1/jobs/audio',b'{"input":"\xff","delay_ns":0}');assert code==400
 code,body=request('POST','/v1/jobs/audio',{'input':'x'});assert code==400
 code,body=request('POST','/v1/jobs/audio',{'input':'x\x00','delay_ns':-1});assert code==400
 code,body=request('POST','/v1/jobs/pipeline',{'input':'字幕 €','delay_ns':0})
 assert code==202,(code,body)
 receipt=json.loads(body);base=f"/v1/jobs/{receipt['epoch']}/{receipt['id']}"
 while True:
  status=get_json(base)
  if status['state'] in ['succeeded','failed','cancelled']:break
  assert time.monotonic()<deadline,status
  time.sleep(.03)
 assert status['state']=='succeeded',status
 assert status['alignment']['sample_frames']==8000,status
 assert status['alignment']['entries'][0]['kind']=='silence',status
 assert 'text_span' not in status['alignment']['entries'][0]
 assert all('integrity' not in item for item in status['artifacts']) and 'error' not in status
 print('pipeline',status['state'],[(a['media_type'],a['byte_length']) for a in status['artifacts']],flush=True)
 for phase in ['text','audio','batch','mux']:
  code,body=request('GET',base+'/artifacts/'+phase,observer=True)
  assert code==200,(phase,code,body)
  if phase=='audio':assert len(body)==16044 and body[:4]==b'RIFF'
  if phase=='mux':
   video=sandbox/'sample.mp4';video.write_bytes(body)
   result=subprocess.run(['/usr/bin/ffprobe','-v','error','-show_entries','stream=codec_type','-of','csv=p=0',str(video)],capture_output=True,text=True,timeout=5)
   assert result.returncode==0 and 'video' in result.stdout and 'audio' in result.stdout,result
   video.unlink()
 code,body=request('GET',base+'/events',observer=True)
 assert code==200 and b'id: ' in body and b'"state":"succeeded"' in body,body
 code,body=request('GET','/v1/live/pcm',observer=True)
 assert code==200 and len(body)==16000 and set(body)=={0}
 code,body=request('POST','/v1/jobs/text',{'input':'late snapshot','delay_ns':1000000000});assert code==202
 late=json.loads(body);late_base=f"/v1/jobs/{late['epoch']}/{late['id']}"
 late_request=urllib.request.Request(f'http://127.0.0.1:{observation}'+late_base+'/events',headers={'X-Owner-Late':'yes'})
 with urllib.request.urlopen(late_request,timeout=2) as response:
  try:late_body=response.read()
  except http.client.IncompleteRead as error:late_body=error.partial
 assert b'id: ' in late_body
 code,body=request('GET','/v1/live/text',observer=True);assert code==200
 code,body=request('POST',late_base+'/cancel');assert code==200
 # Two held SSE connections must leave controller requests available.
 code,body=request('POST','/v1/jobs/text',{'input':'hold','delay_ns':1000000000});assert code==202
 held=json.loads(body);held_base=f"/v1/jobs/{held['epoch']}/{held['id']}"
 url=f'http://127.0.0.1:{observation}'+held_base+'/events'
 first=urllib.request.urlopen(url,timeout=2)
 second=urllib.request.urlopen(url.replace(f":{observation}/",f":{observation_second}/"),timeout=2)
 assert get_json('/health')['accepting']
 queued=[]
 for index in range(4):
  code,body=request('POST','/v1/jobs/text',{'input':'queue','delay_ns':1000000000});assert code==202,(code,body)
  queued.append(json.loads(body))
 before=len(list(root.iterdir()))
 code,body=request('POST','/v1/jobs/text',{'input':'full','delay_ns':0});assert code==429,(code,body)
 assert len(list(root.iterdir()))==before
 cancelled=queued[0];cancel_base=f"/v1/jobs/{cancelled['epoch']}/{cancelled['id']}"
 code,body=request('POST',cancel_base+'/cancel');assert code==200 and json.loads(body)['state']=='cancelled'
 sequence=json.loads(body)['sequence']
 code,body=request('POST',cancel_base+'/cancel');assert json.loads(body)['sequence']==sequence
 code,body=request('POST',held_base+'/cancel');assert code==200
 assert b'"state":"cancelled"' in first.read();first.close()
 assert b'"state":"cancelled"' in second.read();second.close()
 for receipt in queued[1:]:
  job_base=f"/v1/jobs/{receipt['epoch']}/{receipt['id']}"
  request('POST',job_base+'/cancel')
 # Use the private fixture token to model a reader lease, including delayed releases.
 # Production code never exposes this token to external clients.
 code,body=request('POST','/v1/jobs/audio',{'input':'lease','delay_ns':0});assert code==202
 leased=json.loads(body);lease_base=f"/v1/jobs/{leased['epoch']}/{leased['id']}"
 while get_json(lease_base)['state']!='succeeded':
  assert time.monotonic()<deadline;time.sleep(.02)
 private_base=f"/internal/jobs/{leased['epoch']}/{leased['id']}"
 code,body=request('POST',private_base+'/lease/audio',private=True);assert code==200,(code,body)
 grant=json.loads(body)
 code,repeat=request('POST',private_base+'/lease/audio',private=True);assert json.loads(repeat)==grant
 code,discarded=request('POST',private_base+'/lease/audio',private=True);assert code==200
 native_reader=open(root/grant['relative_name'],'rb')
 assert get_json('/health')['active_leases']==1
 latest=None
 for index in range(4):
  if request('GET',lease_base)[0]==404:break
  code,body=request('POST','/v1/jobs/text',{'input':'retire','delay_ns':0});assert code==202
  latest=json.loads(body);latest_base=f"/v1/jobs/{latest['epoch']}/{latest['id']}"
  while get_json(latest_base)['state']!='succeeded':
   assert time.monotonic()<deadline;time.sleep(.02)
 code,body=request('GET',lease_base);assert code==404
 code,replay=request('POST',private_base+'/lease/audio',private=True);assert code==200 and json.loads(replay)==grant
 code,body=request('POST',private_base+'/lease/audio',private=True,slot=1);assert code==404
 assert get_json('/health')['active_leases']==1
 assert (root/grant['relative_name']).exists() and native_reader.read(4)==b'RIFF'
 native_reader.close()
 release_path=f"/internal/slots/0/release/{grant['lease_id']}"
 code,body=request('POST',release_path,private=True);assert code==200
 assert not (root/grant['relative_name']).exists()
 latest_private=f"/internal/jobs/{latest['epoch']}/{latest['id']}"
 code,body=request('POST',latest_private+'/lease/text',private=True);assert code==200
 newer=json.loads(body)
 code,body=request('POST',release_path,private=True);assert code==200
 assert get_json('/health')['active_leases']==1
 assert (root/newer['relative_name']).exists()
 code,body=request('POST',f"/internal/slots/0/release/{newer['lease_id']}",private=True);assert code==200
 assert get_json('/health')['active_leases']==0
 # Future cursors and malformed ids reject before stream commitment.
 code,body=request('GET',latest_base+'/events',observer=True,headers={'Last-Event-ID':'999999'});assert code==400
 code,body=request('GET',f"/v1/jobs/{latest['epoch']}/-1");assert code==400
 # The controlled large reply must time out without occupying control admission.
 stalled=socket.socket();stalled.setsockopt(socket.SOL_SOCKET,socket.SO_RCVBUF,1024)
 stalled.settimeout(2);stalled.connect(('127.0.0.1',control))
 stalled.sendall(b'GET /health HTTP/1.1\r\nHost: local\r\nX-Owner-Stall: yes\r\nConnection: close\r\n\r\n')
 time.sleep(.2);started=time.monotonic();assert get_json('/health')['accepting']
 assert time.monotonic()-started<1;stalled.close()
 assert 'control request failed: Timeout' in log.read_text()
 # A partial client must not stop controller progress.
 partial=socket.create_connection(('127.0.0.1',control),timeout=2)
 partial.sendall(b'POST /v1/jobs/text HTTP/1.1\r\nHost: local\r\nContent-Length: 40\r\n\r\nx')
 time.sleep(.03);assert get_json('/health')['accepting'];partial.close()
 code,body=request('POST','/shutdown',{})
 assert code==400
 code,body=request('POST','/shutdown')
 assert code==202,(code,body)
 server.wait(timeout=5)
 assert server.returncode==0,log.read_text()
 assert list(root.iterdir())==[],list(root.iterdir())
 # Invalid, crashed, oversized and descendant-retaining muxers exercise the same controller.
 tool=sandbox/'mux-fixture';mode_file=sandbox/'mode'
 tool.write_text("#!/usr/bin/python3\nimport os,pathlib,signal,sys,time\nmode=pathlib.Path(__file__).with_name('mode').read_text()\nif mode=='crash':sys.exit(7)\nif mode=='stderr':\n sys.stderr.buffer.write(b'x'*4097);sys.stderr.buffer.flush();sys.stdout.buffer.write(b'\\x00\\x00\\x00\\x0cftypmock');sys.exit(0)\nif mode=='orphan':\n child=os.fork()\n if child==0:\n  signal.signal(signal.SIGTERM,signal.SIG_IGN)\n  os.close(1);os.close(2);time.sleep(60);os._exit(0)\n pathlib.Path(__file__).with_name('descendant').write_text(str(child))\n sys.stdout.buffer.write(b'\\x00\\x00\\x00\\x0cftypmock');sys.stdout.buffer.flush();sys.exit(0)\nif mode=='overflow':\n sys.stdout.buffer.write(b'\\x00\\x00\\x00\\x0cftyp'+b'x'*1048576);sys.stdout.buffer.flush()\nelse:sys.stdout.buffer.write(b'invalid media framing')\n")
 tool.chmod(0o700)
 for mode in ('invalid','crash','overflow','stderr','orphan'):
  mode_file.write_text(mode)
  server=subprocess.Popen([exe,'serve',str(root),str(control),str(observation),str(observation_second),str(tool)],stderr=handle,start_new_session=True)
  while True:
   try:health=get_json('/health');break
   except Exception:
    assert server.poll() is None and time.monotonic()<deadline;time.sleep(.02)
  # Startup refusal preserves the predecessor's fence and cannot disturb its admission.
  refused=subprocess.run([exe,'serve',str(root),str(port()),str(port()),str(port()),str(tool)],capture_output=True,timeout=2)
  assert refused.returncode!=0 and (root/'controller.lock').exists()
  code,body=request('GET',base);assert code==410
  code,body=request('POST','/v1/jobs/pipeline',{'input':'failure','delay_ns':0});assert code==202
  failed=json.loads(body);failed_base=f"/v1/jobs/{failed['epoch']}/{failed['id']}"
  code,body=request('POST','/v1/jobs/text',{'input':'next','delay_ns':0});assert code==202
  next_job=json.loads(body);next_base=f"/v1/jobs/{next_job['epoch']}/{next_job['id']}"
  faulted_seen=False
  while True:
   status=get_json(failed_base);health=get_json('/health')
   if health['device_state']=='faulted':
    faulted_seen=True;assert health['queued']==1
    assert health['running']==failed['id']
   if status['state'] in ('failed','cancelled','succeeded'):break
   if health.get('running')==failed['id']:assert health['queued']==1
   assert time.monotonic()<deadline;time.sleep(.01)
  assert status['state']=='failed' and status['artifacts']==[] and 'alignment' not in status,status
  assert not (root/f"{failed['epoch']}-{failed['id']}-mux").exists()
  if mode=='orphan':
   assert faulted_seen
   descendant=int((sandbox/'descendant').read_text())
   assert not pathlib.Path(f'/proc/{descendant}').exists(),'descendant remained after terminal publication'
  while get_json(next_base)['state']!='succeeded':
   assert time.monotonic()<deadline;time.sleep(.01)
  assert get_json('/health')['reserved_artifact_bytes']<=8*1048576
  code,body=request('POST','/shutdown');assert code==202
  server.wait(timeout=5);assert server.returncode==0,log.read_text()
  assert list(root.iterdir())==[]
  print('mux-failure-control',mode,flush=True)
 server=subprocess.Popen([exe,'serve',str(root),str(control),str(observation),str(observation_second),'/usr/bin/ffmpeg'],stderr=handle,start_new_session=True)
 while True:
  try:health=get_json('/health');break
  except Exception:
   assert server.poll() is None and time.monotonic()<deadline;time.sleep(.02)
 code,body=request('POST','/v1/jobs/audio',{'input':'ambiguous','delay_ns':0});assert code==202
 receipt=json.loads(body);bad_base=f"/v1/jobs/{receipt['epoch']}/{receipt['id']}"
 while get_json(bad_base)['state']!='succeeded':
  assert time.monotonic()<deadline;time.sleep(.01)
 try:request('GET',bad_base+'/artifacts/audio',observer=2);raise AssertionError('malformed applied grant accepted')
 except (urllib.error.URLError,ConnectionError,http.client.RemoteDisconnected):pass
 assert get_json('/health')['active_leases']==1
 request('POST','/shutdown');server.wait(timeout=5)
 assert server.returncode!=0 and list(root.iterdir())==[]
 print('ambiguous-grant-join-ok',flush=True)
 # A forced server death exercises the harness authority across the workers' setsid boundary.
 mode_file.write_text('orphan');(sandbox/'descendant').unlink(missing_ok=True)
 server=subprocess.Popen([exe,'serve',str(root),str(control),str(observation),str(observation_second),str(tool)],stderr=handle,start_new_session=True)
 while True:
  try:get_json('/health');break
  except Exception:
   assert server.poll() is None and time.monotonic()<deadline;time.sleep(.02)
 code,body=request('POST','/v1/jobs/pipeline',{'input':'fallback','delay_ns':0});assert code==202
 while not (sandbox/'descendant').exists():
  assert time.monotonic()<deadline;time.sleep(.01)
 os.killpg(server.pid,signal.SIGKILL);server.wait(timeout=2)
 assert (root/'controller.lock').is_file()
 reaped=reap_owned();assert reaped>0
 print('forced-descendant-reap-ok',reaped,flush=True)
 # Only the external harness's completed child reap permits manual fixture reconciliation.
 for entry in root.iterdir():entry.unlink()
 print('reference-integration-ok',flush=True)
finally:
 signal.alarm(0)
 for stream in (first,second):
  if stream is not None:stream.close()
 if native_reader is not None:native_reader.close()
 if server is not None and server.poll() is None:
  try:
   request('POST','/shutdown');server.wait(timeout=3)
  except Exception:
   os.killpg(server.pid,signal.SIGKILL);server.wait(timeout=2)
 recovered=reap_owned()
 assert recovered==0,'unexpected native leftovers after owner execution'
 assert libc.prctl(36,prior_subreaper.value,0,0,0)==0
 signal.signal(signal.SIGCHLD,prior_sigchld)
 handle.close()
 print(log.read_text(),flush=True)
 if list(root.iterdir())==[]:
  shutil.rmtree(sandbox)
 else:print('unclean fixture preserved at',sandbox,flush=True)
"###;
