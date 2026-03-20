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

struct DashboardPayload: Decodable {
    let generatedAt: String
    let portfolio: PortfolioPayload
    let analytics: AnalyticsPayload
    let monitoring: MonitoringPayload
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

struct MonitoringPayload: Decodable {
    let latestTimeframeSignal: LatestSignalPayload?
    let technicalSignalCount: Int
    let triggeredAlertCount: Int
    let marketPulse: [MarketPulsePayload]
    let watchlist: [WatchlistPayload]
    let news: [NewsPayload]
    let system: SystemSnapshotPayload
}

struct LatestSignalPayload: Decodable {
    let signalType: String
    let severity: String
    let description: String
    let detectedAt: String
}

struct MarketPulsePayload: Decodable, Identifiable {
    var id: String { symbol }
    let symbol: String
    let name: String
    let value: FlexValue?
    let dayChangePct: FlexValue?
}

struct WatchlistPayload: Decodable, Identifiable {
    var id: String { symbol }
    let symbol: String
    let name: String
    let category: String
    let currentPrice: FlexValue?
    let dayChangePct: FlexValue?
    let targetPrice: FlexValue?
    let distancePct: FlexValue?
    let targetHit: Bool
}

struct NewsPayload: Decodable, Identifiable {
    var id: String { "\(source)-\(publishedAt)-\(title)" }
    let title: String
    let source: String
    let publishedAt: String
    let sourceType: String
}

struct SystemSnapshotPayload: Decodable {
    let daemon: DaemonPayload
    let sources: [SourceStatusPayload]
}

struct DaemonPayload: Decodable {
    let running: Bool
    let status: String
    let lastHeartbeat: String?
}

struct SourceStatusPayload: Decodable, Identifiable {
    var id: String { name }
    let name: String
    let status: String
    let freshness: String
    let lastFetch: String?
    let records: Int
}
