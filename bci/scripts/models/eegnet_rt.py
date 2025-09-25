#!/usr/bin/env python3
# eegnet_rt.py  (Py3.9+)
import argparse, asyncio, contextlib, json, struct, time, math, pathlib
from typing import Any, Dict, Iterable, List, Optional, Tuple, Union

import numpy as np
import torch
from torch.utils.data import TensorDataset, DataLoader
from braindecode.models import EEGNetv4
from scipy.signal import butter, sosfiltfilt, iirnotch, filtfilt, welch

import websockets

# -------------------- config --------------------
WINDOW_S = 1.25          # window for MI
HOP_S    = 0.25
BP = (1.0, 45.0)
NOTCHS = [(60.0, 40.0), (120.0, 40.0)]  # (f0, Q)
LABEL_SHIFT_S = 0.6
EDGE_TRIM_S   = 0.4
BATCH = 64
LR = 1e-3
EPOCHS = 30

DEFAULT_CHANNELS_HINT = ["ch1","ch2","ch3","ch4","ch5","ch6","ch7","ch8"]

# -------------------- cleaning --------------------
_sos_cache: Dict[float, np.ndarray] = {}
def bp_filter(x: np.ndarray, fs: float) -> np.ndarray:
    sos = _sos_cache.get(fs)
    if sos is None:
        sos = butter(4, [BP[0]/(fs/2), BP[1]/(fs/2)], btype="band", output="sos")
        _sos_cache[fs] = sos
    return sosfiltfilt(sos, x, axis=1)

def iir_notch_zero_phase(x, fs, f0, q):
    b,a = iirnotch(w0=f0, Q=q, fs=fs)
    return filtfilt(b, a, x, axis=1)

def clean(x: np.ndarray, fs: float) -> np.ndarray:
    y = bp_filter(x, fs)
    for f0,q in NOTCHS:
        y = iir_notch_zero_phase(y, fs, f0, q)
    return y

# -------------------- features (for artifact gate only) --------------------
def window_power(x: np.ndarray) -> float:
    return float(np.sum(x**2))

# -------------------- Output WS --------------------
class OutputHub:
    def __init__(self): self._clients=set()
    async def register(self, ws): self._clients.add(ws)
    async def unregister(self, ws): self._clients.discard(ws)
    async def broadcast(self, obj):
        if not self._clients: return
        msg=json.dumps(obj); dead=[]
        for c in list(self._clients):
            try: await c.send(msg)
            except Exception: dead.append(c)
        for d in dead: await self.unregister(d)

async def output_server(hub, host, port):
    async def handler(ws):
        await hub.register(ws)
        try: await ws.wait_closed()
        finally: await hub.unregister(ws)
    async with websockets.serve(handler, host, port, max_size=2*1024*1024):
        print(f"[infer] Output WS on ws://{host}:{port}/output")
        await asyncio.Future()

# -------------------- DataHub source (same wire spec as yours) --------------------
MetaMap = Dict[str, Dict[str, Any]]
def ws_data_to_np_array(header: Dict[str, Any], payload: bytes) -> np.ndarray:
    dtype = "<i4" if header.get("packet_type")=="RawI32" else "<f4"
    data = np.frombuffer(payload, dtype=dtype)
    ch = header.get("num_channels", 0) or 1
    bs = header.get("batch_size", 0) or (len(data)//ch)
    return data.reshape(bs, ch, order="C")

def ws_data_received(message: Union[str, bytes], metadata: MetaMap) -> Tuple[str, Any]:
    if isinstance(message, str):
        obj=json.loads(message)
        if obj.get("message_type")=="meta_update":
            metadata[obj["topic"]] = obj["meta"]; return "meta", obj
        return "control", obj
    view=memoryview(message)
    (header_len,) = struct.unpack(">I", view[:4])
    header = json.loads(view[4:4+header_len].tobytes())
    payload = view[4+header_len:].tobytes()
    samples = ws_data_to_np_array(header, payload)
    meta = metadata.get(header["topic"]) if isinstance(metadata, dict) else None
    return "samples", (header, samples, meta)

async def start_data_ws(host: str, subscriptions: Iterable[Tuple[str,int]], on_packet):
    url=f"ws://{host}:9000/ws/data"; metadata: MetaMap={}
    async with websockets.connect(url, max_size=8*1024*1024) as ws:
        for topic,epoch in subscriptions:
            await ws.send(json.dumps({"type":"subscribe","topic":topic,"epoch":epoch}))
        async for message in ws:
            kind,payload = ws_data_received(message, metadata)
            if kind=="samples":
                header,samples,meta = payload
                r = on_packet(header, samples, meta)
                if asyncio.iscoroutine(r): await r

async def datahub_chunk_iter(host:str, topic:str, epoch:int, fs_override:Optional[float], ch_override:Optional[List[str]]):
    q: asyncio.Queue = asyncio.Queue(maxsize=128)
    async def on_packet(header, samples: np.ndarray, meta):
        x = samples.T.copy()
        t_recv = time.time()
        fs=None; chs=None
        if meta:
            fs = float(meta.get("fs")) if meta.get("fs") is not None else None
            chs = meta.get("ch_names") or meta.get("channels")
        if fs is None: fs = fs_override or 250.0
        if chs is None: chs = ch_override or [f"ch{i}" for i in range(x.shape[0])]
        await q.put((x, fs, t_recv, chs))
    feeder = asyncio.create_task(start_data_ws(host, [(topic, epoch)], on_packet))
    try:
        while True: yield await q.get()
    finally:
        feeder.cancel()
        with contextlib.suppress(asyncio.CancelledError): await feeder

# -------------------- utils --------------------
def choose_channels(ch_names: List[str]) -> List[int]:
    idx = [ch_names.index(c) for c in DEFAULT_CHANNELS_HINT if c in ch_names]
    return idx if idx else list(range(len(ch_names)))

class WindowCollector:
    def __init__(self, minutes: float, block_s: float, label_shift_s=0.6, edge_trim_s=0.4,
                 window_s=1.25, hop_s=0.25):
        self.minutes = minutes
        self.block_s = block_s
        self.label_shift_s = label_shift_s
        self.edge_trim_s = edge_trim_s
        self.window_s = window_s
        self.hop_s = hop_s

        total_s = int(minutes * 60)
        blocks = []
        cur = 0.0
        lab = 1
        while cur < total_s:
            blocks.append((cur + label_shift_s, cur + block_s + label_shift_s, lab))
            cur += block_s
            lab = 1 - lab
        self.blocks = blocks

        self.X: List[np.ndarray] = []
        self.y: List[int] = []
        self.block_ids: List[int] = []

        self.fs = None
        self.chs = None
        self.sel_idx = None
        self.ring = None
        self.ring_len = 0
        self.write_pos = 0
        self.samples_seen = 0
        self.t0 = time.time()
        self.last_emit = self.t0
        self.pow_hist: List[float] = []
        self.done = False

    def process(self, data_tuple):
        if self.done:
            return True
        x, fs_msg, t_recv, ch_names = data_tuple
        if self.fs is None:
            self.fs = fs_msg
            self.chs = ch_names
            self.sel_idx = [i for i in range(len(ch_names))]
            self.ring_len = int(3 * self.fs)
            self.ring = np.zeros((len(self.sel_idx), self.ring_len), np.float64)
            print("[collect] fs", self.fs, "using", [self.chs[i] for i in self.sel_idx])

        sel = x[self.sel_idx, :]
        n = sel.shape[1]
        for i in range(n):
            self.ring[:, self.write_pos] = sel[:, i]
            self.write_pos = (self.write_pos + 1) % self.ring_len
        self.samples_seen += n

        now = time.time()
        if now - self.last_emit >= self.hop_s and self.fs is not None:
            self.last_emit = now
            m = int(self.window_s * self.fs)
            if m <= self.ring_len and self.samples_seen >= m:
                idxs = (np.arange(m) + self.write_pos - m) % self.ring_len
                xw = self.ring[:, idxs]
                rel_t = now - self.t0

                bidx = blab = bstart = bend = None
                for k, (s, e, l) in enumerate(self.blocks):
                    if s <= rel_t <= e:
                        bidx, blab, bstart, bend = k, l, s, e
                        break
                if bidx is None:
                    self.done = True
                    return True

                if rel_t < (bstart + self.edge_trim_s) or rel_t > (bend - self.edge_trim_s):
                    return False

                xf = clean(xw, self.fs)
                p = float(np.sum(xf ** 2))
                self.pow_hist.append(p)
                med = np.median(self.pow_hist) if self.pow_hist else 0.0
                if med > 0 and (p > 5 * med or p < 0.1 * med):
                    return False

                self.X.append(xf.astype("float32"))
                self.y.append(int(blab))
                self.block_ids.append(int(bidx))

        if now - self.t0 > (self.blocks[-1][1] + self.edge_trim_s + 1.0):
            self.done = True
            return True
        return False

# -------------------- train --------------------
async def train_datahub(host, topic, epoch, out_dir, minutes, block_s, fs_override=None, channels=None):
    outp=pathlib.Path(out_dir); outp.mkdir(parents=True, exist_ok=True)
    chunker = datahub_chunk_iter(host, topic, epoch, fs_override, channels.split(",") if channels else None)

    collector = WindowCollector(minutes, block_s,
                                label_shift_s=LABEL_SHIFT_S, edge_trim_s=EDGE_TRIM_S,
                                window_s=WINDOW_S, hop_s=HOP_S)

    async for data in chunker:
        finished = collector.process(data)
        if finished:
            break

    if not collector.X:
        print("[train] no samples"); return
    X=np.stack(collector.X)           # (N,C,T)
    y=np.array(collector.y, np.int64)
    blk=np.array(collector.block_ids, np.int64)
    print("[train] X",X.shape,"y",y.shape,"blocks",len(set(blk.tolist())))

    # block-wise split
    uniq=sorted(set(blk.tolist())); cut=max(1,int(round(len(uniq)*0.8)))
    tr_blocks=uniq[:cut]; te_blocks=uniq[cut:]
    tr=np.isin(blk,tr_blocks); te=np.isin(blk,te_blocks)
    Xtr, ytr = X[tr], y[tr]; Xte, yte = X[te], y[te]
    print("[train] split:",Xtr.shape,Xte.shape)

    # model
    n_ch, n_t = X.shape[1], X.shape[2]
    model = EEGNetv4(n_chans=n_ch, n_outputs=2, input_window_samples=n_t, final_conv_length="auto")
    device="cuda" if torch.cuda.is_available() else "cpu"
    model.to(device)
    opt = torch.optim.Adam(model.parameters(), lr=LR, weight_decay=1e-4)
    loss_fn = torch.nn.CrossEntropyLoss()

    tr_loader=DataLoader(TensorDataset(torch.from_numpy(Xtr), torch.from_numpy(ytr)), batch_size=BATCH, shuffle=True, drop_last=True)
    te_loader=DataLoader(TensorDataset(torch.from_numpy(Xte), torch.from_numpy(yte)), batch_size=BATCH, shuffle=False)

    def run_epoch(loader, train=True):
        model.train(train)
        tot=0; correct=0; loss_sum=0.0
        for xb,yb in loader:
            xb, yb = xb.to(device), yb.to(device)
            if train: opt.zero_grad()
            out = model(xb)
            loss = loss_fn(out, yb)
            if train:
                loss.backward()
                torch.nn.utils.clip_grad_norm_(model.parameters(), 5.0)
                opt.step()
            pred = out.argmax(1)
            tot += yb.numel(); correct += int((pred==yb).sum())
            loss_sum += loss.item()*yb.numel()
        return correct/max(1,tot), loss_sum/max(1,tot)

    for ep in range(1, EPOCHS+1):
        ta, tl = run_epoch(tr_loader, True)
        va, vl = run_epoch(te_loader, False)
        print(f"[train] epoch {ep:02d}  train {ta:.3f}/{tl:.3f}  val {va:.3f}/{vl:.3f}")

    torch.save({"state_dict": model.state_dict(), "n_ch": n_ch, "n_t": n_t}, outp/"eegnet.pt")
    print("[train] saved", outp/"eegnet.pt")

# -------------------- infer --------------------
async def infer_datahub(host, topic, epoch, model_path, out_host, out_port, fs_override=None, channels=None):
    ckpt = torch.load(model_path, map_location="cpu")
    n_ch, n_t = int(ckpt["n_ch"]), int(ckpt["n_t"])
    model = EEGNetv4(n_chans=n_ch, n_outputs=2, input_window_samples=n_t, final_conv_length="auto")
    model.load_state_dict(ckpt["state_dict"]); model.eval()
    device="cuda" if torch.cuda.is_available() else "cpu"; model.to(device)

    hub=OutputHub()
    server_task=asyncio.create_task(output_server(hub, out_host, out_port))

    fs=None; chs=None; sel_idx=None
    ring=None; ring_len=0; write_pos=0; last_emit=time.time()

    alpha = 1 - math.exp(-HOP_S/0.4)  # EMA tau ~0.4s
    p_up_s = 0.5; state="down"

    chunker = datahub_chunk_iter(host, topic, epoch, fs_override, channels.split(",") if channels else None)
    try:
        async for data, fs_msg, t_recv, ch_names in chunker:
            if fs is None:
                fs=fs_msg; chs=ch_names; sel_idx=choose_channels(chs)
                ring_len=int(3*fs); ring=np.zeros((len(sel_idx), ring_len), np.float32)
                print("[infer] fs",fs,"using", [chs[i] for i in sel_idx])
            sel=data[sel_idx,:].astype(np.float32)
            n=sel.shape[1]
            for i in range(n):
                ring[:,write_pos]=sel[:,i]; write_pos=(write_pos+1)%ring_len

            now=time.time()
            if now-last_emit>=HOP_S and fs is not None:
                last_emit=now; m=int(WINDOW_S*fs)
                if m<=ring_len:
                    idxs=(np.arange(m)+write_pos-m)%ring_len
                    xw=ring[:,idxs]
                    xf=clean(xw, fs).astype(np.float32)
                    # shape (1,C,T)
                    xb = torch.from_numpy(xf[None,...]).to(device)
                    with torch.no_grad():
                        prob = torch.softmax(model(xb), dim=-1)[0].cpu().numpy()
                    up = float(prob[1]); p_up_s = (1-alpha)*p_up_s + alpha*up
                    if state!="up" and p_up_s>0.65: state="up"
                    elif state!="down" and p_up_s<0.35: state="down"
                    await hub.broadcast({"t": time.time(), "y": state, "p": {"up": up, "down": float(prob[0])}, "latency_ms": int((time.time()-t_recv)*1000)})
    finally:
        server_task.cancel()
        with contextlib.suppress(asyncio.CancelledError): await server_task

# -------------------- CLI --------------------
def main():
    ap = argparse.ArgumentParser()
    sub = ap.add_subparsers(dest="cmd", required=True)

    tr = sub.add_parser("train-datahub")
    tr.add_argument("--host", required=True); tr.add_argument("--topic", default="eeg_voltage"); tr.add_argument("--epoch", type=int, default=1)
    tr.add_argument("--out", default="./eegnet_model")
    tr.add_argument("--minutes", type=float, default=5.0); tr.add_argument("--block", type=float, default=5.0)
    tr.add_argument("--fs", type=float, default=None); tr.add_argument("--channels", type=str, default=None)

    inf = sub.add_parser("infer-datahub")
    inf.add_argument("--host", required=True); inf.add_argument("--topic", default="eeg_voltage"); inf.add_argument("--epoch", type=int, default=1)
    inf.add_argument("--model", required=True)
    inf.add_argument("--out-host", default="127.0.0.1"); inf.add_argument("--out-port", type=int, default=8766)
    inf.add_argument("--fs", type=float, default=None); inf.add_argument("--channels", type=str, default=None)

    a=ap.parse_args()
    if a.cmd=="train-datahub":
        asyncio.run(train_datahub(a.host, a.topic, a.epoch, a.out, a.minutes, a.block, a.fs, a.channels)); return
    if a.cmd=="infer-datahub":
        asyncio.run(infer_datahub(a.host, a.topic, a.epoch, a.model, a.out_host, a.out_port, a.fs, a.channels)); return

if __name__=="__main__": main()
