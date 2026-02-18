import Foundation

struct ChatMessage: Identifiable, Equatable {
    let id: String
    let role: MessageRole
    let content: String
    let timestamp: Date
    var isStreaming: Bool

    init(
        id: String = UUID().uuidString,
        role: MessageRole,
        content: String,
        timestamp: Date = .now,
        isStreaming: Bool = false
    ) {
        self.id = id
        self.role = role
        self.content = content
        self.timestamp = timestamp
        self.isStreaming = isStreaming
    }

    static func == (lhs: ChatMessage, rhs: ChatMessage) -> Bool {
        lhs.id == rhs.id && lhs.content == rhs.content && lhs.isStreaming == rhs.isStreaming
    }
}

enum MessageRole: String, Codable {
    case user
    case assistant
    case system
    case tool

    var isUser: Bool { self == .user }
}
