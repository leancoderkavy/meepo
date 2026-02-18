import Foundation

struct MeepoSession: Identifiable, Codable {
    let id: String
    let name: String
    let agentId: String?
    let kind: String?
    let createdAt: String?
    let lastActivity: String?
    let messageCount: Int?

    enum CodingKeys: String, CodingKey {
        case id, name, kind
        case agentId = "agent_id"
        case createdAt = "created_at"
        case lastActivity = "last_activity"
        case messageCount = "message_count"
    }

    var displayName: String {
        name.isEmpty ? id : name
    }

    var formattedLastActivity: String? {
        guard let lastActivity else { return nil }
        let formatter = ISO8601DateFormatter()
        formatter.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        guard let date = formatter.date(from: lastActivity) else { return lastActivity }
        let relative = RelativeDateTimeFormatter()
        relative.unitsStyle = .abbreviated
        return relative.localizedString(for: date, relativeTo: .now)
    }
}

struct StatusResponse: Codable {
    let status: String
    let sessions: Int
    let connectedClients: Int
    let uptimeSecs: Int

    enum CodingKeys: String, CodingKey {
        case status, sessions
        case connectedClients = "connected_clients"
        case uptimeSecs = "uptime_secs"
    }

    var formattedUptime: String {
        let hours = uptimeSecs / 3600
        let minutes = (uptimeSecs % 3600) / 60
        if hours > 0 {
            return "\(hours)h \(minutes)m"
        }
        return "\(minutes)m"
    }
}
