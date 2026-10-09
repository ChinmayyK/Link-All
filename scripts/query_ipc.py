import socket, json
s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
s.connect('/tmp/linkall.sock')
s.sendall(json.dumps({"req_id": "1", "payload": {"Status": null}}).encode() + b'\n')
data = b""
while True:
    chunk = s.recv(4096)
    if not chunk: break
    data += chunk
print(data.decode())
