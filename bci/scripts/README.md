## cwt_live.py
`python cwt_live.py --host raspberrypi.local --topic eeg_voltage --epoch 1 --fs 250 --window 12`

## print_data.py
`python print_data.py`

## graph2.py
`python graph2.py --host raspberrypi.local --topic eeg_voltage --epoch 1`

## metabci.py
```python
# Train 
python metabci.py train-datahub --host raspberrypi.local --topic eeg_voltage --epoch 1

# Infer (game listens at ws://127.0.0.1:8766/output)
python metabci.py infer-datahub --host raspberrypi.local --topic eeg_voltage --epoch 1
python bci_print.py
```

## How to make a new script

Use ws_client.py
```python
async def start_data_ws(
    host: str,
    subscriptions: Iterable[Tuple[str, int]],
    on_packet: PacketHandler,
) -> None:
```
