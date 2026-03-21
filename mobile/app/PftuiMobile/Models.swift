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
    let catalysts: CatalystsPayload
    let impact: ImpactPayload
    let opportunities: OpportunitiesPayload
    let narrative: NarrativePayload
    let synthesis: SynthesisPayload
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

struct CatalystsPayload: Decodable {
    let window: String
    let label: String
    let generatedAt: String
    let catalysts: [CatalystEventPayload]
}

struct CatalystEventPayload: Decodable, Identifiable {
    var id: String { "\(time)-\(title)" }
    let title: String
    let time: String
    let source: String
    let category: String
    let significance: String
    let countdownBucket: String
    let affectedAssets: [String]
    let linkedScenarios: [String]
    let linkedPredictions: [String]
    let portfolioRelevance: Int
    let macroSignificance: Int
    let score: Int
    let detail: String
}

struct ImpactPayload: Decodable {
    let generatedAt: String
    let exposures: [AssetInsightPayload]
}

struct OpportunitiesPayload: Decodable {
    let generatedAt: String
    let opportunities: [AssetInsightPayload]
}

struct NarrativePayload: Decodable {
    let generatedAt: String
    let requestedDate: String
    let sourceDate: String
    let headline: String
    let subtitle: String
    let coverageNote: String?
    let recap: NarrativeRecapPayload
    let scenarioShifts: [ScenarioShiftPayload]
    let convictionChanges: [ConvictionShiftPayload]
    let trendChanges: [TrendShiftPayload]
    let predictionScorecard: PredictionScorecardPayload
    let surprises: [NarrativeInsightPayload]
    let lessons: [LessonPayload]
    let catalystOutcomes: [CatalystOutcomePayload]
}

struct NarrativeRecapPayload: Decodable {
    let date: String
    let note: String?
    let events: [NarrativeRecapEventPayload]
    let count: Int
}

struct NarrativeRecapEventPayload: Decodable, Identifiable {
    var id: String { "\(at)-\(eventType)-\(source)" }
    let at: String
    let eventType: String
    let source: String
    let summary: String
}

struct ScenarioShiftPayload: Decodable, Identifiable {
    var id: String { "\(name)-\(updatedAt)" }
    let name: String
    let previousProbability: Double
    let currentProbability: Double
    let deltaPct: Double
    let driver: String?
    let updatedAt: String
    let severity: String
}

struct ConvictionShiftPayload: Decodable, Identifiable {
    var id: String { "\(symbol)-\(updatedAt)" }
    let symbol: String
    let name: String
    let oldScore: Int
    let newScore: Int
    let delta: Int
    let updatedAt: String
    let notes: String?
    let severity: String
}

struct TrendShiftPayload: Decodable, Identifiable {
    var id: String { "\(name)-\(updatedAt)" }
    let name: String
    let timeframe: String
    let direction: String
    let conviction: String
    let updatedAt: String
    let latestEvidence: String?
    let affectedAssets: [String]
    let severity: String
}

struct PredictionScorecardPayload: Decodable {
    let total: Int
    let scored: Int
    let pending: Int
    let correct: Int
    let partial: Int
    let wrong: Int
    let hitRatePct: Double
    let recentResolutions: [PredictionResolutionPayload]
}

struct PredictionResolutionPayload: Decodable, Identifiable {
    var id: Int { Int(self.rawId) }
    private let rawId: Int64
    let claim: String
    let symbol: String?
    let outcome: String
    let lesson: String?
    let scoredAt: String
    let severity: String

    private enum CodingKeys: String, CodingKey {
        case rawId = "id"
        case claim, symbol, outcome, lesson, scoredAt, severity
    }
}

struct NarrativeInsightPayload: Decodable, Identifiable {
    var id: String { "\(title)-\(value)" }
    let title: String
    let detail: String
    let value: String
    let severity: String
}

struct LessonPayload: Decodable, Identifiable {
    var id: String { "\(title)-\(recordedAt)" }
    let title: String
    let detail: String
    let symbol: String?
    let recordedAt: String
    let severity: String
}

struct CatalystOutcomePayload: Decodable, Identifiable {
    var id: String { "\(date)-\(title)" }
    let title: String
    let date: String
    let category: String
    let linkedAssets: [String]
    let outcome: String
    let detail: String
    let severity: String
}

struct AssetInsightPayload: Decodable, Identifiable {
    var id: String { symbol }
    let symbol: String
    let name: String
    let held: Bool
    let watchlist: Bool
    let allocationPct: String?
    let currentValue: String?
    let consensus: String
    let score: Int
    let severity: String
    let summary: String
    let evidenceChain: [String]
}

struct SynthesisPayload: Decodable {
    let generatedAt: String
    let strongestAlignment: [AlignmentStatePayload]
    let highestConfidenceDivergence: [DivergenceStatePayload]
    let constraintFlows: [ConstraintStatePayload]
    let unresolvedTensions: [SynthesisNotePayload]
    let watchTomorrow: [WatchTomorrowPayload]
}

struct AlignmentStatePayload: Decodable, Identifiable {
    var id: String { symbol }
    let symbol: String
    let name: String
    let low: String
    let medium: String
    let high: String
    let macroBias: String
    let consensus: String
    let scorePct: Double
    let bullLayers: Int
    let bearLayers: Int
}

struct DivergenceStatePayload: Decodable, Identifiable {
    var id: String { symbol }
    let symbol: String
    let name: String
    let low: String
    let medium: String
    let high: String
    let macroBias: String
    let dominantSide: String
    let disagreementPct: Double
    let summary: String
}

struct ConstraintStatePayload: Decodable, Identifiable {
    var id: String { "\(fromTimeframe)-\(toTimeframe)-\(title)" }
    let title: String
    let fromTimeframe: String
    let toTimeframe: String
    let direction: String
    let severity: String
    let summary: String
}

struct SynthesisNotePayload: Decodable, Identifiable {
    var id: String { "\(title)-\(severity)" }
    let title: String
    let detail: String
    let severity: String
}

struct WatchTomorrowPayload: Decodable, Identifiable {
    var id: String { symbol }
    let symbol: String
    let name: String
    let reason: String
    let trigger: String
    let severity: String
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
