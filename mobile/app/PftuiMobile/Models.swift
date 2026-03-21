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
    let situation: SituationPayload
    let deltas: DeltasPayload
}

struct SituationPayload: Decodable {
    let title: String
    let subtitle: String
    let summary: [SituationStatPayload]
    let watchNow: [SituationInsightPayload]
    let portfolioImpacts: [SituationInsightPayload]
    let riskMatrix: [RiskSignalPayload]
}

struct DeltasPayload: Decodable {
    let window: String
    let label: String
    let currentAt: String
    let baselineAt: String?
    let coverage: String
    let changeRadar: [SituationInsightPayload]
}

struct SituationStatPayload: Decodable, Identifiable {
    var id: String { label }
    let label: String
    let value: String
}

struct SituationInsightPayload: Decodable, Identifiable {
    var id: String { "\(title)-\(value)" }
    let title: String
    let detail: String
    let value: String
    let severity: String
}

struct RiskSignalPayload: Decodable, Identifiable {
    var id: String { label }
    let label: String
    let detail: String
    let value: String
    let status: String
    let severity: String
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
    let regime: RegimePayload?
    let correlations: [CorrelationPayload]
    let sentiment: [SentimentPayload]
    let predictions: [PredictionPayload]
}

struct TimeframePayload: Decodable, Identifiable {
    var id: String { timeframe }
    let timeframe: String
    let label: String
    let score: Double
    let summary: String?
    let updatedAt: String?
}

struct RegimePayload: Decodable {
    let regime: String
    let confidence: Double?
    let drivers: [String]
    let recordedAt: String
    let vix: Double?
    let dxy: Double?
    let yield10y: Double?
    let oil: Double?
    let gold: Double?
    let btc: Double?
}

struct CorrelationPayload: Decodable, Identifiable {
    var id: String { "\(symbolA)-\(symbolB)-\(period)" }
    let symbolA: String
    let symbolB: String
    let correlation: Double
    let period: String
    let recordedAt: String
}

struct SentimentPayload: Decodable, Identifiable {
    var id: String { indexType }
    let indexType: String
    let value: Int
    let classification: String
    let updatedAt: String
}

struct PredictionPayload: Decodable, Identifiable {
    var id: String { question }
    let question: String
    let probabilityPct: Double
    let category: String
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
    let server: ServerRuntimePayload
    let database: DatabaseHealthPayload
    let daemon: DaemonPayload
    let sources: [SourceStatusPayload]
}

struct ServerRuntimePayload: Decodable {
    let pftuiVersion: String
    let backend: String
    let portfolioMode: String
    let databaseMode: String
    let mobilePort: Int
    let apiTokenCount: Int
    let sessionTtlHours: Int
}

struct DatabaseHealthPayload: Decodable {
    let status: String
    let label: String
    let integrity: String
    let positions: Int
    let transactions: Int
    let watchlist: Int
    let trackedPrices: Int
    let staleSources: Int
    let lastMarketSync: String?
    let lastNewsSync: String?
}

struct DaemonPayload: Decodable {
    let running: Bool
    let status: String
    let cycle: Int
    let lastHeartbeat: String?
    let lastRefreshDurationSecs: Double?
    let intervalSecs: Int
    let taskCount: Int
    let errorCount: Int
    let tasks: [String]
}

struct SourceStatusPayload: Decodable, Identifiable {
    var id: String { name }
    let name: String
    let status: String
    let freshness: String
    let lastFetch: String?
    let records: Int
}
