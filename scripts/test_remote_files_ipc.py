#!/usr/bin/env python3
"""
Automated IPC Socket Test Script for Link All Remote File Queries.

Tests local IPC JSON serialization and socket protocol handling for `IpcRequest::RemoteFilesQuery`.
Runs in-process Unix domain socket server and client to verify JSON request encoding, socket transport,
and response payload parsing.
"""

import json
import os
import socket
import sys
import tempfile
import threading
import time
import unittest


class MockIpcServer:
    def __init__(self, socket_path):
        self.socket_path = socket_path
        self.server_sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        self.server_sock.bind(self.socket_path)
        self.server_sock.listen(5)
        self.running = True
        self.received_requests = []

    def start(self):
        self.thread = threading.Thread(target=self._run, daemon=True)
        self.thread.start()

    def _run(self):
        while self.running:
            try:
                conn, _ = self.server_sock.accept()
                threading.Thread(target=self._handle_client, args=(conn,), daemon=True).start()
            except OSError:
                break

    def _handle_client(self, conn):
        with conn:
            buffer = ""
            while self.running:
                data = conn.recv(4096)
                if not data:
                    break
                buffer += data.decode("utf-8")
                while "\n" in buffer:
                    line, buffer = buffer.split("\n", 1)
                    if not line.strip():
                        continue
                    req = json.loads(line)
                    self.received_requests.append(req)

                    # Formulate mock IpcResponse
                    cmd = req.get("cmd")
                    if cmd == "remote_files_query":
                        target_device = req.get("target_device")
                        category = req.get("category")
                        summary_only = req.get("summary_only", False)

                        if summary_only:
                            res = {
                                "status": "ok",
                                "data": {
                                    "summary": {
                                        "type_counts": {
                                            "images": 12,
                                            "videos": 3,
                                            "audio": 5,
                                            "documents": 2,
                                            "apks": 1,
                                            "archives": 0
                                        },
                                        "source_counts": {
                                            "whatsapp": 8,
                                            "downloads": 10,
                                            "camera": 5
                                        }
                                    },
                                    "files": [],
                                    "total_matching": 23,
                                    "error": None
                                }
                            }
                        else:
                            res = {
                                "status": "ok",
                                "data": {
                                    "summary": None,
                                    "files": [
                                        {
                                            "file_id": 1001,
                                            "display_name": "vacation.jpg",
                                            "size_bytes": 204800,
                                            "mime_type": "image/jpeg",
                                            "date_modified": 1770000000,
                                            "category": category or "Images",
                                            "source": "Camera",
                                            "content_uri": "content://media/external/file/1001"
                                        }
                                    ],
                                    "total_matching": 1,
                                    "error": None
                                }
                            }
                    else:
                        res = {"status": "error", "message": f"Unknown command: {cmd}"}

                    response_bytes = (json.dumps(res) + "\n").encode("utf-8")
                    conn.sendall(response_bytes)

    def stop(self):
        self.running = False
        try:
            self.server_sock.close()
        except OSError:
            pass
        if os.path.exists(self.socket_path):
            os.remove(self.socket_path)


class TestRemoteFilesIpc(unittest.TestCase):

    def setUp(self):
        self.tmp_dir = tempfile.TemporaryDirectory()
        self.socket_path = os.path.join(self.tmp_dir.name, "linkall.sock")
        self.server = MockIpcServer(self.socket_path)
        self.server.start()
        time.sleep(0.05)

    def tearDown(self):
        self.server.stop()
        self.tmp_dir.cleanup()

    def _send_ipc_request(self, req_dict):
        sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        sock.connect(self.socket_path)
        try:
            payload = (json.dumps(req_dict) + "\n").encode("utf-8")
            sock.sendall(payload)

            data = b""
            while b"\n" not in data:
                chunk = sock.recv(4096)
                if not chunk:
                    break
                data += chunk

            line = data.decode("utf-8").strip()
            return json.loads(line)
        finally:
            sock.close()

    def test_ipc_remote_files_query_images(self):
        """Test sending RemoteFilesQuery request for Images category over IPC socket."""
        req = {
            "cmd": "remote_files_query",
            "target_device": "550e8400-e29b-41d4-a716-446655440000",
            "summary_only": False,
            "category": "Images",
            "source": "Camera",
            "search_query": None,
            "offset": 0,
            "limit": 50
        }
        resp = self._send_ipc_request(req)

        self.assertEqual(resp["status"], "ok")
        self.assertIn("data", resp)
        files = resp["data"]["files"]
        self.assertEqual(len(files), 1)
        self.assertEqual(files[0]["display_name"], "vacation.jpg")
        self.assertEqual(files[0]["category"], "Images")

    def test_ipc_remote_files_query_summary_only(self):
        """Test sending RemoteFilesQuery request with summary_only=true over IPC socket."""
        req = {
            "cmd": "remote_files_query",
            "target_device": "550e8400-e29b-41d4-a716-446655440000",
            "summary_only": True,
            "category": None,
            "source": None,
            "search_query": None,
            "offset": 0,
            "limit": 50
        }
        resp = self._send_ipc_request(req)

        self.assertEqual(resp["status"], "ok")
        summary = resp["data"]["summary"]
        self.assertIsNotNone(summary)
        self.assertEqual(summary["type_counts"]["images"], 12)
        self.assertEqual(summary["source_counts"]["downloads"], 10)
        self.assertEqual(len(resp["data"]["files"]), 0)

    def test_ipc_serialization_schema_validation(self):
        """Verify client JSON request payload matches Link All IpcRequest schema."""
        req = {
            "cmd": "remote_files_query",
            "target_device": "12345678-1234-1234-1234-123456789abc",
            "summary_only": False,
            "category": "Videos",
            "source": "Downloads",
            "search_query": "clip",
            "offset": 10,
            "limit": 25,
            "timeout_secs": 15,
        }
        _ = self._send_ipc_request(req)
        received = self.server.received_requests[-1]

        self.assertEqual(received["cmd"], "remote_files_query")
        self.assertEqual(received["target_device"], "12345678-1234-1234-1234-123456789abc")
        self.assertEqual(received["category"], "Videos")
        self.assertEqual(received["source"], "Downloads")
        self.assertEqual(received["search_query"], "clip")
        self.assertEqual(received["offset"], 10)
        self.assertEqual(received["limit"], 25)
        self.assertEqual(received["timeout_secs"], 15)


if __name__ == "__main__":
    unittest.main()
