import websocket
import threading

def on_message(ws, message):
    print(f"Received: {message}")

def on_error(ws, error):
    print(f"Error: {error}")

def on_close(ws, close_status_code, close_msg):
    print("### closed ###")

def on_open(ws):
    print("Opened connection")
    # You might need to send a subscription message here
    # ws.send('{"subscribe": "eeg_data"}')

if __name__ == "__main__":
    # Replace with your Raspberry Pi's IP
    ws_url = "ws://<RASPBERRY_PI_IP_ADDRESS>:9000/ws/data"
    
    ws = websocket.WebSocketApp(ws_url,
                              on_open=on_open,
                              on_message=on_message,
                              on_error=on_error,
                              on_close=on_close)

    ws.run_forever()
