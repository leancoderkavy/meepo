import Foundation
import Combine

enum ConnectionState: Equatable {
    case disconnected
    case connecting
    case connected
    case error(String)

    var isConnected: Bool {
        if case .connected = self { return true }
        return false
    }

    var statusText: String {
        switch self {
        case .disconnected: return "Disconnected"
        case .connecting: return "Connecting…"
        case .connected: return "Connected"
        case .error(let msg): return "Error: \(msg)"
        }
    }
}

@MainActor
final class MeepoClient: ObservableObject {
    @Published var connectionState: ConnectionState = .disconnected
    @Published var isTyping: Bool = false
    @Published var currentTool: String? = nil

    private var webSocketTask: URLSessionWebSocketTask?
    private var session: URLSession
    private var pendingRequests: [String: CheckedContinuation<GatewayResponse, Error>] = [:]
    private var timeoutTasks: [String: Task<Void, Never>] = [:]
    private var eventHandlers: [(GatewayEvent) -> Void] = []
    private var reconnectTask: Task<Void, Never>?
    private var receiveTask: Task<Void, Never>?
    private var pingTask: Task<Void, Never>?
    private var reconnectAttempts: Int = 0

    private let settings: SettingsStore

    init(settings: SettingsStore) {
        self.settings = settings
        let config = URLSessionConfiguration.default
        config.timeoutIntervalForRequest = 5
        self.session = URLSession(configuration: config)
    }

    // MARK: - Connection

    func connect() {
        reconnectAttempts = 0
        performConnect()
    }

    private func performConnect() {
        guard !connectionState.isConnected else { return }

        // Clean up any previous connection state
        teardownConnection()

        connectionState = .connecting

        guard let url = gatewayWebSocketURL() else {
            connectionState = .error("Invalid URL")
            return
        }

        var request = URLRequest(url: url)
        request.timeoutInterval = 5
        if !settings.authToken.isEmpty {
            request.setValue("Bearer \(settings.authToken)", forHTTPHeaderField: "Authorization")
        }

        let ws = session.webSocketTask(with: request)
        webSocketTask = ws
        ws.resume()

        // The receive loop validates the connection: if the first receive
        // succeeds, we're connected. If it fails, we transition to error.
        // We also start a timeout that fires if no message arrives in 5s.
        startReceiving()
        startPingLoop()

        // Connection validation timeout — if still .connecting after 5s,
        // the server is unreachable.
        Task { [weak self] in
            try? await Task.sleep(for: .seconds(5))
            guard let self, self.connectionState == .connecting else { return }
            self.connectionState = .error("Cannot reach gateway")
            self.webSocketTask?.cancel(with: .abnormalClosure, reason: nil)
            self.webSocketTask = nil
            self.receiveTask?.cancel()
            self.pingTask?.cancel()
            self.scheduleReconnect()
        }
    }

    func disconnect() {
        reconnectAttempts = 0
        teardownConnection()
        connectionState = .disconnected
    }

    private func teardownConnection() {
        reconnectTask?.cancel()
        reconnectTask = nil
        receiveTask?.cancel()
        receiveTask = nil
        pingTask?.cancel()
        pingTask = nil

        webSocketTask?.cancel(with: .normalClosure, reason: nil)
        webSocketTask = nil
        isTyping = false
        currentTool = nil

        // Cancel all timeout tasks
        for (_, task) in timeoutTasks {
            task.cancel()
        }
        timeoutTasks.removeAll()

        // Fail all pending requests
        for (_, continuation) in pendingRequests {
            continuation.resume(throwing: MeepoError.disconnected)
        }
        pendingRequests.removeAll()
    }

    // MARK: - Send message to agent

    func sendMessage(_ content: String, sessionId: String = "main") async throws -> String {
        let response = try await sendRequest(
            method: GatewayMethod.messageSend,
            params: [
                "content": AnyCodable(content),
                "session_id": AnyCodable(sessionId)
            ]
        )

        guard let result = response.result?.dictValue,
              let responseContent = result["content"] as? String else {
            throw MeepoError.invalidResponse
        }
        return responseContent
    }

    // MARK: - Sessions

    func listSessions() async throws -> [MeepoSession] {
        let response = try await sendRequest(method: GatewayMethod.sessionList)
        guard let data = try? JSONSerialization.data(
            withJSONObject: response.result?.value ?? [],
            options: []
        ) else {
            return []
        }
        return (try? JSONDecoder().decode([MeepoSession].self, from: data)) ?? []
    }

    func createSession(name: String) async throws -> MeepoSession {
        let response = try await sendRequest(
            method: GatewayMethod.sessionNew,
            params: ["name": AnyCodable(name)]
        )
        guard let data = try? JSONSerialization.data(
            withJSONObject: response.result?.value ?? [:],
            options: []
        ) else {
            throw MeepoError.invalidResponse
        }
        return try JSONDecoder().decode(MeepoSession.self, from: data)
    }

    // MARK: - Status (REST)

    func fetchStatus() async throws -> StatusResponse {
        guard let url = gatewayHTTPURL(path: "/api/status") else {
            throw MeepoError.invalidURL
        }
        var request = URLRequest(url: url)
        if !settings.authToken.isEmpty {
            request.setValue("Bearer \(settings.authToken)", forHTTPHeaderField: "Authorization")
        }
        let (data, response) = try await session.data(for: request)
        guard let http = response as? HTTPURLResponse, http.statusCode == 200 else {
            throw MeepoError.httpError
        }
        return try JSONDecoder().decode(StatusResponse.self, from: data)
    }

    // MARK: - Event subscription

    func onEvent(_ handler: @escaping (GatewayEvent) -> Void) {
        eventHandlers.append(handler)
    }

    // MARK: - Private

    private func sendRequest(
        method: String,
        params: [String: AnyCodable] = [:]
    ) async throws -> GatewayResponse {
        guard connectionState.isConnected, let ws = webSocketTask else {
            throw MeepoError.disconnected
        }

        let request = GatewayRequest(method: method, params: params)
        let requestId = request.id ?? UUID().uuidString

        let data = try JSONEncoder().encode(request)
        guard let jsonString = String(data: data, encoding: .utf8) else {
            throw MeepoError.encodingError
        }

        try await ws.send(.string(jsonString))

        return try await withCheckedThrowingContinuation { continuation in
            pendingRequests[requestId] = continuation

            // Timeout after 30 seconds — cancellable so it doesn't leak
            let timeoutTask = Task {
                try? await Task.sleep(for: .seconds(30))
                guard !Task.isCancelled else { return }
                if let pending = pendingRequests.removeValue(forKey: requestId) {
                    timeoutTasks.removeValue(forKey: requestId)
                    pending.resume(throwing: MeepoError.timeout)
                }
            }
            timeoutTasks[requestId] = timeoutTask
        }
    }

    private func startReceiving() {
        receiveTask?.cancel()
        receiveTask = Task { [weak self] in
            guard let self else { return }
            var receivedFirst = false
            while !Task.isCancelled {
                guard let ws = self.webSocketTask else { break }
                do {
                    let message = try await ws.receive()

                    // First successful receive confirms the connection is live
                    if !receivedFirst {
                        receivedFirst = true
                        await MainActor.run {
                            if self.connectionState == .connecting {
                                self.connectionState = .connected
                                self.reconnectAttempts = 0
                            }
                        }
                    }

                    switch message {
                    case .string(let text):
                        await self.handleIncoming(text)
                    case .data(let data):
                        if let text = String(data: data, encoding: .utf8) {
                            await self.handleIncoming(text)
                        }
                    @unknown default:
                        break
                    }
                } catch {
                    if !Task.isCancelled {
                        await MainActor.run {
                            let msg = receivedFirst ? "Connection lost" : "Cannot reach gateway"
                            self.connectionState = .error(msg)
                            self.scheduleReconnect()
                        }
                    }
                    break
                }
            }
        }
    }

    private func handleIncoming(_ text: String) async {
        guard let data = text.data(using: .utf8),
              let message = IncomingMessage.decode(from: data) else { return }

        switch message {
        case .response(let response):
            // Direct response (not used by current gateway, but future-proof)
            if let id = response.id,
               let continuation = pendingRequests.removeValue(forKey: id) {
                timeoutTasks.removeValue(forKey: id)?.cancel()
                continuation.resume(returning: response)
            }

        case .event(let event):
            handleEvent(event)
        }
    }

    private func handleEvent(_ event: GatewayEvent) {
        switch event.event {
        case GatewayEventName.typingStart:
            isTyping = true
            currentTool = nil
        case GatewayEventName.typingStop:
            isTyping = false
            currentTool = nil
        case GatewayEventName.toolExecuting:
            if let dict = event.data.dictValue, let tool = dict["tool"] as? String {
                currentTool = tool
            }
        case "response":
            // The gateway broadcasts responses as events (ws_sender is moved
            // into the send_task, so handle_request can't reply directly).
            // Structure: {"event": "response", "data": {"id": ..., "result": ..., "error": ...}}
            if let dict = event.data.dictValue,
               let id = dict["id"] as? String,
               let continuation = pendingRequests.removeValue(forKey: id) {
                timeoutTasks.removeValue(forKey: id)?.cancel()
                let result = dict["result"].map { AnyCodable($0) }
                let errorDict = dict["error"] as? [String: Any]
                let gatewayError = errorDict.map {
                    GatewayError(
                        code: $0["code"] as? Int ?? -1,
                        message: $0["message"] as? String ?? "Unknown error"
                    )
                }
                let response = GatewayResponse(id: id, result: result, error: gatewayError)
                continuation.resume(returning: response)
                return
            }
        default:
            break
        }

        for handler in eventHandlers {
            handler(event)
        }
    }

    private func startPingLoop() {
        pingTask?.cancel()
        pingTask = Task { [weak self] in
            while !Task.isCancelled {
                try? await Task.sleep(for: .seconds(30))
                guard let ws = self?.webSocketTask else { break }
                ws.sendPing { error in
                    if error != nil {
                        Task { @MainActor in
                            self?.connectionState = .error("Ping failed")
                        }
                    }
                }
            }
        }
    }

    private func scheduleReconnect() {
        guard reconnectAttempts < 10 else {
            // Give up after 10 attempts — user can manually retry
            connectionState = .error("Cannot reach gateway")
            return
        }
        reconnectTask?.cancel()
        let attempt = reconnectAttempts
        reconnectTask = Task { [weak self] in
            // Exponential backoff: 2s, 4s, 8s, 16s, max 30s
            let delay = min(30.0, 2.0 * pow(2.0, Double(attempt)))
            try? await Task.sleep(for: .seconds(delay))
            guard !Task.isCancelled else { return }
            await MainActor.run {
                guard let self else { return }
                self.reconnectAttempts += 1
                self.performConnect()
            }
        }
    }

    // MARK: - URL builders

    private func gatewayWebSocketURL() -> URL? {
        let scheme = settings.useTLS ? "wss" : "ws"
        return URL(string: "\(scheme)://\(settings.host):\(settings.port)/ws")
    }

    private func gatewayHTTPURL(path: String) -> URL? {
        let scheme = settings.useTLS ? "https" : "http"
        return URL(string: "\(scheme)://\(settings.host):\(settings.port)\(path)")
    }
}

// MARK: - Errors

enum MeepoError: LocalizedError {
    case disconnected
    case invalidURL
    case invalidResponse
    case httpError
    case encodingError
    case timeout
    case serverError(String)

    var errorDescription: String? {
        switch self {
        case .disconnected: return "Not connected to Meepo"
        case .invalidURL: return "Invalid gateway URL"
        case .invalidResponse: return "Invalid response from server"
        case .httpError: return "HTTP request failed"
        case .encodingError: return "Failed to encode request"
        case .timeout: return "Request timed out"
        case .serverError(let msg): return msg
        }
    }
}
