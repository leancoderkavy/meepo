import Foundation
import SwiftUI

@MainActor
final class ChatViewModel: ObservableObject {
    @Published var messages: [ChatMessage] = []
    @Published var inputText: String = ""
    @Published var isSending: Bool = false
    @Published var errorMessage: String? = nil
    @Published var currentSessionId: String = "main"
    @Published var sessions: [MeepoSession] = []

    // Track content we've already displayed to prevent duplicates from
    // the gateway broadcasting both a response AND a message.received event.
    private var recentContents: Set<String> = []

    let client: MeepoClient
    let settings: SettingsStore

    init(client: MeepoClient, settings: SettingsStore) {
        self.client = client
        self.settings = settings
        setupEventHandlers()
    }

    // MARK: - Actions

    func send() {
        let text = inputText.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !text.isEmpty, !isSending else { return }

        inputText = ""
        errorMessage = nil

        let userMessage = ChatMessage(role: .user, content: text)
        messages.append(userMessage)

        isSending = true

        Task {
            do {
                let response = try await client.sendMessage(text, sessionId: currentSessionId)
                // Mark this content so the broadcast event is suppressed
                let dedupKey = "assistant:\(response)"
                recentContents.insert(dedupKey)
                let assistantMessage = ChatMessage(role: .assistant, content: response)
                messages.append(assistantMessage)
            } catch {
                errorMessage = error.localizedDescription
                let errorMsg = ChatMessage(
                    role: .system,
                    content: "⚠ \(error.localizedDescription)"
                )
                messages.append(errorMsg)
            }
            isSending = false
        }
    }

    func connect() {
        client.connect()
    }

    func disconnect() {
        client.disconnect()
    }

    func loadSessions() {
        Task {
            do {
                sessions = try await client.listSessions()
            } catch {
                errorMessage = error.localizedDescription
            }
        }
    }

    func createSession(name: String) {
        Task {
            do {
                let session = try await client.createSession(name: name)
                sessions.insert(session, at: 0)
                switchSession(to: session.id)
            } catch {
                errorMessage = error.localizedDescription
            }
        }
    }

    func switchSession(to sessionId: String) {
        currentSessionId = sessionId
        messages.removeAll()
        recentContents.removeAll()
    }

    func clearMessages() {
        messages.removeAll()
        recentContents.removeAll()
        errorMessage = nil
    }

    // MARK: - Event handling

    private func setupEventHandlers() {
        client.onEvent { [weak self] event in
            Task { @MainActor in
                self?.handleEvent(event)
            }
        }
    }

    private func handleEvent(_ event: GatewayEvent) {
        switch event.event {
        case GatewayEventName.messageReceived:
            guard let dict = event.data.dictValue,
                  let content = dict["content"] as? String,
                  let sessionId = dict["session_id"] as? String,
                  sessionId == currentSessionId else { return }

            let role = (dict["role"] as? String).flatMap { MessageRole(rawValue: $0) } ?? .assistant

            // Robust dedup: skip if we already displayed this content from sendMessage
            let dedupKey = "\(role.rawValue):\(content)"
            if recentContents.contains(dedupKey) {
                recentContents.remove(dedupKey)
                return
            }

            let message = ChatMessage(role: role, content: content)
            messages.append(message)

        case GatewayEventName.sessionCreated:
            loadSessions()

        default:
            break
        }
    }
}
