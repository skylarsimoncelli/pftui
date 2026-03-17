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
    let totalCost: FlexValue
    let totalGain: FlexValue?
    let totalGainPct: FlexValue?
    let dailyChange: FlexValue?
    let dailyChangePct: FlexValue?
    let positions: [PositionPayload]
}

struct PositionPayload: Decodable, Identifiable {
    var id: String { symbol }
    let symbol: String
    let name: String
    let category: String
    let quantity: FlexValue
    let avgCost: FlexValue
    let totalCost: FlexValue
    let currency: String
    let currentPrice: FlexValue?
    let currentValue: FlexValue?
    let gain: FlexValue?
    let gainPct: FlexValue?
    let allocationPct: FlexValue?
}

struct AnalyticsPayload: Decodable {
    let summary: SummaryPayload
    let macroView: MacroPayload
    let performance: PerformancePayload
}

struct SummaryPayload: Decodable {
    let totalValue: FlexValue?
    let positionCount: Int
    let topMovers: [PositionPayload]
}

struct MacroPayload: Decodable {
    let indicators: [MacroIndicatorPayload]
    let topMovers: [MacroIndicatorPayload]
}

struct MacroIndicatorPayload: Decodable, Identifiable {
    var id: String { symbol }
    let symbol: String
    let name: String
    let value: FlexValue?
    let changePct: FlexValue?
}

struct PerformancePayload: Decodable {
    let dailyValues: [PortfolioValuePoint]
    let metrics: PerformanceMetricsPayload
    let estimated: Bool
    let source: String
}

struct PortfolioValuePoint: Decodable, Identifiable {
    var id: String { date }
    let date: String
    let value: FlexValue
}

struct PerformanceMetricsPayload: Decodable {
    let totalReturnPct: FlexValue?
    let maxDrawdownPct: FlexValue?
}
