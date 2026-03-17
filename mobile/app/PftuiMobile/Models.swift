import Foundation

struct FlexValue: Codable, Hashable {
    let raw: String

    init(from decoder: Decoder) throws {
        let container = try decoder.singleValueContainer()
        if let string = try? container.decode(String.self) {
            self.raw = string
        } else if let intValue = try? container.decode(Int.self) {
            self.raw = String(intValue)
        } else if let doubleValue = try? container.decode(Double.self) {
            self.raw = String(format: "%.2f", doubleValue)
        } else if let boolValue = try? container.decode(Bool.self) {
            self.raw = boolValue ? "true" : "false"
        } else {
            self.raw = "—"
        }
    }

    func encode(to encoder: Encoder) throws {
        var container = encoder.singleValueContainer()
        try container.encode(raw)
    }
}

struct ConnectionSettings: Codable, Equatable {
    var server: String
    var fingerprint: String
    var token: String
}

struct PortfolioPayload: Decodable {
    let totalValue: FlexValue?
    let dailyChangePct: FlexValue?
    let positionCount: Int
    let positions: [PositionPayload]
}

struct PositionPayload: Decodable, Identifiable {
    var id: String { symbol }
    let symbol: String
    let name: String
    let category: String
    let currentPrice: FlexValue?
    let currentValue: FlexValue?
    let allocationPct: FlexValue?
    let dayChangePct: FlexValue?
}

struct AnalyticsPayload: Decodable {
    let timeframes: [TimeframePayload]
}

struct TimeframePayload: Decodable, Identifiable {
    var id: String { timeframe }
    let timeframe: String
    let label: String
    let score: Double
    let summary: String?
    let updatedAt: String?
}
