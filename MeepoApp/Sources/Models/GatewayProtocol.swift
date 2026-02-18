import Foundation

// MARK: - Client → Gateway

struct GatewayRequest: Codable {
    let method: String
    var params: [String: AnyCodable]
    var id: String?

    init(method: String, params: [String: AnyCodable] = [:], id: String? = UUID().uuidString) {
        self.method = method
        self.params = params
        self.id = id
    }
}

// MARK: - Gateway → Client (response)

struct GatewayResponse: Codable {
    let id: String?
    let result: AnyCodable?
    let error: GatewayError?

    var isSuccess: Bool { error == nil }
}

struct GatewayError: Codable {
    let code: Int
    let message: String
}

// MARK: - Gateway → Client (event)

struct GatewayEvent: Codable {
    let event: String
    let data: AnyCodable
}

// MARK: - Well-known methods

enum GatewayMethod {
    static let messageSend = "message.send"
    static let sessionList = "session.list"
    static let sessionNew = "session.new"
    static let sessionHistory = "session.history"
    static let statusGet = "status.get"
}

// MARK: - Well-known events

enum GatewayEventName {
    static let messageReceived = "message.received"
    static let typingStart = "typing.start"
    static let typingStop = "typing.stop"
    static let toolExecuting = "tool.executing"
    static let statusUpdate = "status.update"
    static let sessionCreated = "session.created"
}

// MARK: - Incoming message envelope (response OR event)

enum IncomingMessage {
    case response(GatewayResponse)
    case event(GatewayEvent)

    static func decode(from data: Data) -> IncomingMessage? {
        let decoder = JSONDecoder()
        // Try event first (has "event" key)
        if let event = try? decoder.decode(GatewayEvent.self, from: data),
           !event.event.isEmpty {
            return .event(event)
        }
        // Try response
        if let response = try? decoder.decode(GatewayResponse.self, from: data) {
            return .response(response)
        }
        return nil
    }
}

// MARK: - AnyCodable (type-erased JSON value)

struct AnyCodable: Codable, @unchecked Sendable {
    let value: Any

    init(_ value: Any) {
        self.value = value
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.singleValueContainer()
        if container.decodeNil() {
            value = NSNull()
        } else if let bool = try? container.decode(Bool.self) {
            value = bool
        } else if let int = try? container.decode(Int.self) {
            value = int
        } else if let double = try? container.decode(Double.self) {
            value = double
        } else if let string = try? container.decode(String.self) {
            value = string
        } else if let array = try? container.decode([AnyCodable].self) {
            value = array.map(\.value)
        } else if let dict = try? container.decode([String: AnyCodable].self) {
            value = dict.mapValues(\.value)
        } else {
            throw DecodingError.dataCorruptedError(in: container, debugDescription: "Unsupported type")
        }
    }

    func encode(to encoder: Encoder) throws {
        var container = encoder.singleValueContainer()
        switch value {
        case is NSNull:
            try container.encodeNil()
        case let bool as Bool:
            try container.encode(bool)
        case let int as Int:
            try container.encode(int)
        case let double as Double:
            try container.encode(double)
        case let string as String:
            try container.encode(string)
        case let array as [Any]:
            try container.encode(array.map { AnyCodable($0) })
        case let dict as [String: Any]:
            try container.encode(dict.mapValues { AnyCodable($0) })
        default:
            try container.encodeNil()
        }
    }

    // Convenience accessors
    var stringValue: String? { value as? String }
    var intValue: Int? { value as? Int }
    var doubleValue: Double? { value as? Double }
    var boolValue: Bool? { value as? Bool }
    var dictValue: [String: Any]? { value as? [String: Any] }
    var arrayValue: [Any]? { value as? [Any] }
}
