import Foundation
import Network



private final class ResumeGuard: @unchecked Sendable {
    private let lock = NSLock()
    private var hasResumed = false

    func tryResume() -> Bool {
        lock.lock()
        defer { lock.unlock() }
        if hasResumed { return false }
        hasResumed = true
        return true
    }

    func revert() {
        lock.lock()
        hasResumed = false
        lock.unlock()
    }
}

actor PersistentIPCConnection {
    private var connection: NWConnection?
    private let socketPath: String
    /// Longest a request may wait for its reply. The slowest commands wait on
    /// another device (a remote files query gives up after 10 s), so this only
    /// trips when the daemon itself has stopped answering.
    private let requestTimeout: TimeInterval
    private var requestQueue: [CheckedContinuation<Void, Never>] = []
    private var isExecutingRequest = false
    
    init(socketPath: String, requestTimeout: TimeInterval = 30) {
        self.socketPath = socketPath
        self.requestTimeout = requestTimeout
    }
    
    private func acquireLock() async {
        if !isExecutingRequest {
            isExecutingRequest = true
            return
        }
        await withCheckedContinuation { continuation in
            requestQueue.append(continuation)
        }
    }

    private func releaseLock() {
        if !requestQueue.isEmpty {
            let next = requestQueue.removeFirst()
            next.resume()
        } else {
            isExecutingRequest = false
        }
    }
    
    private func ensureConnection() async throws -> NWConnection {
        if let conn = connection, conn.state == .ready {
            return conn
        }
        
        connection?.cancel()
        
        let endpoint = NWEndpoint.unix(path: socketPath)
        let parameters = NWParameters.tcp
        let newConnection = NWConnection(to: endpoint, using: parameters)
        
        return try await withCheckedThrowingContinuation { continuation in
            let guardObj = ResumeGuard()
            DispatchQueue.global().asyncAfter(deadline: .now() + 0.5) {
                if guardObj.tryResume() {
                    newConnection.cancel()
                    continuation.resume(throwing: LinkAllIPCError.connectionFailed)
                }
            }
            newConnection.stateUpdateHandler = { [weak self, weak newConnection] state in
                guard let newConnection = newConnection else { return }
                guard guardObj.tryResume() else {
                    if case .failed = state {
                        newConnection.cancel()
                        Task { await self?.clearConnectionIfCurrent(newConnection) }
                    }
                    return
                }
                
                switch state {
                case .ready:
                    continuation.resume(returning: newConnection)
                case .failed(let error):
                    newConnection.cancel()
                    continuation.resume(throwing: error)
                case .cancelled:
                    continuation.resume(throwing: LinkAllIPCError.disconnected)
                default:
                    guardObj.revert()
                }
            }
            newConnection.start(queue: .global())
            self.connection = newConnection
        }
    }
    
    func send(cmd: [String: Any]) async throws -> Data {
        await acquireLock()
        defer { releaseLock() }
        
        // A daemon that accepts but never answers would otherwise hold this
        // request, and every one queued behind it, forever. Cancelling the
        // connection fails the pending send or receive.
        let timedOut = ResumeGuard()
        var timer: DispatchWorkItem?
        defer { timer?.cancel() }

        do {
            let conn = try await ensureConnection()
            let deadline = DispatchWorkItem {
                if timedOut.tryResume() { conn.cancel() }
            }
            timer = deadline
            DispatchQueue.global().asyncAfter(deadline: .now() + requestTimeout, execute: deadline)
            let payload = try JSONSerialization.data(withJSONObject: cmd) + Data("\n".utf8)
            
            try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<Void, Error>) in
                conn.send(content: payload, completion: .contentProcessed { error in
                    if let error = error {
                        continuation.resume(throwing: error)
                    } else {
                        continuation.resume()
                    }
                })
            }
            
            return try await receiveLine(from: conn)
        } catch {
            connection?.cancel()
            connection = nil
            // Once the deadline has fired, any error here came from cancelling.
            if !timedOut.tryResume() { throw LinkAllIPCError.timedOut }
            throw error
        }
    }
    
    private func receiveLine(from conn: NWConnection) async throws -> Data {
        var response = Data()
        while true {
            let chunk = try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<Data, Error>) in
                conn.receive(minimumIncompleteLength: 1, maximumLength: 4096) { data, _, isComplete, error in
                    if let error = error {
                        continuation.resume(throwing: error)
                    } else if let data = data {
                        continuation.resume(returning: data)
                    } else if isComplete {
                        continuation.resume(throwing: LinkAllIPCError.disconnected)
                    } else {
                        continuation.resume(throwing: LinkAllIPCError.noData)
                    }
                }
            }
            response.append(chunk)
            if response.last == UInt8(ascii: "\n") {
                break
            }
        }
        return response
    }
    
    private func clearConnectionIfCurrent(_ conn: NWConnection) {
        if self.connection === conn {
            self.connection = nil
        }
    }
}
