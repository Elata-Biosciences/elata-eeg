# tongue training

PYTHONPATH=. python3 bci/scripts/models/train.py --model mi_tongue --npz /Users/khan/Projects/elata-eeg/bci/scripts/data/session_emg_20250930_155728.npz --chs 1,5 --window_s 1.0 --hop_s 0.25 --epochs 12 --batch 64 --lr 3e-3 --save /tmp/emg_lr_rest.pt

alias bci_record='python3 bci/scripts/gather_data/record_epochs.py --host 172.20.10.12 --topic eeg_voltage --epoch 1 --out bci/scripts/data/session.npz --labels "left,right,up,down" --cued --minutes 3 --block 4 --label-shift 0.6'

alias bci_contact_check='python3 bci/scripts/explore/contact_check.py'

#PYTHONPATH=. python3 bci/scripts/models/train.py --model mi_tongue --npz bci/scripts/data/session_emg_9-30-3-52-pm.npz --chs 1,5 --window_s 1.0 --hop_s 0.25 --epochs 12 --batch 64 --lr 3e-3 --save /tmp/emg_click.pt

#PYTHONPATH=. python3 bci/scripts/gather_data/record_epochs.py  --host 172.20.10.12 --topic eeg_voltage --epoch 1 --out bci/scripts/data/session_emg_$(date +%Y%m%d_%H%M%S).npz --labels "left,right,rest" --cued --minutes 4  --block 3 --label-shift 0.4

PYTHONPATH=. python3 bci/scripts/explore/cwt_live.py --host 172.20.10.12 --topic eeg_voltage --epoch 1 --nch 4 --grid-cols 1 --window 4 --fmin 1 --fmax 45 --zscore

HOST=172.20.10.12 LABELS="left,right,up,down" MINUTES=5 BLOCK=5 \
LABEL_SHIFT=0.2 CHS="2,3,4" FREQS="15,12,10,7.5" \
INVERT=1 FULLSCREEN=1 MODE=sine CONTRAST=0.35 COUNTDOWN=3 \
./run bci_ssvep_record_py

NPZ=$(ls -t bci/scripts/data/session_*.npz | head -n1) && \
PYTHONPATH=. python3 bci/scripts/models/train.py \
  --model gpt1 --npz "$NPZ" --chs 2,3,4 \
  --window_s 1.0 --hop_s 0.25 \
  --epochs 40 --batch 128 --lr 1e-3 \
  --norm dataset \
  --save /tmp/ssvep_4way.pt

FREQS="15,12,10,7.5" LABELS="left,right,up,down" CHS="all" \
WINDOW_S=1.0 HOP_S=0.25 HARMONICS=3 \
bash scripts/bci_ssvep_decode.sh

# eyebrows
PYTHONPATH=. python3 bci/scripts/gather_data/record_epochs.py \
  --host 172.20.10.12 --topic eeg_voltage --epoch 1 \
  --out bci/scripts/data/session_emg_$(date +%Y%m%d_%H%M%S).npz \
  --labels "left,right,rest" --cued --minutes 4 --block 3 --label-shift 0.3 --beep

NPZ=$(ls -t bci/scripts/data/session_*.npz | head -n1) && \
PYTHONPATH=. python3 bci/scripts/models/train.py \
  --model gpt1 --npz "$NPZ" --chs 1,2,3,4,5,6,7,8 \
  --window_s 1.0 --hop_s 0.25 \
  --epochs 40 --batch 128 --lr 1e-3 \
  --norm dataset \
  --save /tmp/eyebrows_left_right_rest.pt
