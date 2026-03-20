import SwiftUI
#if canImport(UIKit)
import UIKit
#else
typealias UIKeyboardType = Int
extension UIKeyboardType {
    static let `default` = 0
}
#endif

private enum MobilePalette {
    static let bgPrimary = Color(red: 13/255, green: 17/255, blue: 23/255)
    static let bgSecondary = Color(red: 22/255, green: 27/255, blue: 34/255)
    static let bgTertiary = Color(red: 33/255, green: 38/255, blue: 45/255)
    static let bgAccent = Color(red: 20/255, green: 27/255, blue: 39/255)
    static let border = Color(red: 48/255, green: 54/255, blue: 61/255)
    static let textPrimary = Color(red: 201/255, green: 209/255, blue: 217/255)
    static let textSecondary = Color(red: 139/255, green: 148/255, blue: 158/255)
    static let textTertiary = Color(red: 110/255, green: 118/255, blue: 129/255)
    static let accent = Color(red: 137/255, green: 220/255, blue: 235/255)
    static let blue = Color(red: 137/255, green: 180/255, blue: 250/255)
    static let green = Color(red: 166/255, green: 227/255, blue: 161/255)
    static let amber = Color(red: 249/255, green: 226/255, blue: 175/255)
    static let red = Color(red: 243/255, green: 139/255, blue: 168/255)
}

struct RootView: View {
    @EnvironmentObject private var store: MobileStore
    @State private var selectedTab = 0

    var body: some View {
        Group {
            if store.connection == nil {
                SetupView()
            } else {
                DashboardShellView(selectedTab: $selectedTab)
                    .task {
                        if store.dashboard == nil {
                            await store.refresh()
                        }
                    }
            }
        }
        .background(MobilePalette.bgPrimary.ignoresSafeArea())
    }
}

struct SetupView: View {
    @EnvironmentObject private var store: MobileStore
    @State private var server = ""
    @State private var apiToken = ""
    @State private var fingerprint = ""

    var body: some View {
        NavigationStack {
            ScrollView {
                VStack(alignment: .leading, spacing: 24) {
                    VStack(alignment: .leading, spacing: 12) {
                        Text("pftui mobile")
                            .font(.system(size: 36, weight: .bold, design: .rounded))
                            .foregroundStyle(MobilePalette.textPrimary)
                        Text("A sleek remote client for the pftui database. Monitor portfolio state, analytics, watchlist pressure, signals, and data freshness from one screen.")
                            .foregroundStyle(MobilePalette.textSecondary)
                    }

                    card {
                        VStack(alignment: .leading, spacing: 14) {
                            AppField(title: "Server", text: $server, placeholder: "192.168.1.10:9443")
                            AppField(title: "API Token", text: $apiToken, placeholder: "pftm_read_...", secure: true)
                            AppField(title: "Fingerprint", text: $fingerprint, placeholder: "AA:BB:CC:...")
                            Button {
                                Task {
                                    await store.connect(server: server, apiToken: apiToken, fingerprint: fingerprint)
                                }
                            } label: {
                                Text(store.isBusy ? "Connecting…" : "Connect")
                                    .frame(maxWidth: .infinity)
                            }
                            .buttonStyle(PrimaryButtonStyle())
                        }
                    }

                    if let error = store.errorMessage {
                        Text(error)
                            .foregroundStyle(MobilePalette.red)
                            .font(.footnote)
                    }

                    card {
                        VStack(alignment: .leading, spacing: 10) {
                            Text("Server setup")
                                .foregroundStyle(MobilePalette.textPrimary)
                                .font(.headline)
                            Text("1. Run `pftui system mobile enable --bind 0.0.0.0` once")
                            Text("2. Run `pftui system mobile token generate --permission read --name ios`")
                            Text("3. Start `pftui system mobile serve`")
                            Text("4. Enter the host, token, and fingerprint shown by the server")
                        }
                        .font(.footnote)
                        .foregroundStyle(MobilePalette.textSecondary)
                    }
                }
                .padding(20)
            }
        }
    }
}

struct DashboardShellView: View {
    @EnvironmentObject private var store: MobileStore
    @Binding var selectedTab: Int

    var body: some View {
        TabView(selection: $selectedTab) {
            NavigationStack {
                HomeView()
            }
            .tabItem {
                Label("Home", systemImage: "waveform.path.ecg")
            }
            .tag(0)

            NavigationStack {
                PortfolioView()
            }
            .tabItem {
                Label("Portfolio", systemImage: "briefcase.fill")
            }
            .tag(1)

            NavigationStack {
                AnalyticsView()
            }
            .tabItem {
                Label("Analytics", systemImage: "chart.line.uptrend.xyaxis")
            }
            .tag(2)
        }
        .tint(MobilePalette.accent)
    }
}

struct HomeView: View {
    @EnvironmentObject private var store: MobileStore
    @AppStorage("pftui.mobile.maskValues") private var maskValues = false

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 18) {
                topBar(maskValues: $maskValues)

                heroCard(
                    title: masked(store.portfolio?.totalValue?.raw),
                    subtitle: "Remote Portfolio",
                    detail: "\(store.portfolio?.positionCount ?? 0) positions"
                )

                if let monitoring = store.dashboard?.monitoring {
                    signalSummaryCard(monitoring: monitoring)

                    if !monitoring.marketPulse.isEmpty {
                        sectionTitle("Market Pulse")
                        card {
                            VStack(spacing: 12) {
                                ForEach(monitoring.marketPulse) { item in
                                    compactRow(
                                        title: item.symbol,
                                        subtitle: item.name,
                                        trailing: item.dayChangePct?.raw ?? item.value?.raw ?? "—",
                                        trailingColor: deltaColor(item.dayChangePct?.raw)
                                    )
                                }
                            }
                        }
                    }

                    if !monitoring.watchlist.isEmpty {
                        sectionTitle("Watchlist")
                        card {
                            VStack(spacing: 12) {
                                ForEach(monitoring.watchlist.prefix(5)) { item in
                                    compactRow(
                                        title: item.symbol,
                                        subtitle: item.distancePct.map { "Target \($0.raw) away" } ?? item.category,
                                        trailing: item.dayChangePct?.raw ?? item.currentPrice?.raw ?? "—",
                                        trailingColor: deltaColor(item.dayChangePct?.raw)
                                    )
                                }
                            }
                        }
                    }

                    sectionTitle("System")
                    card {
                        VStack(alignment: .leading, spacing: 14) {
                            HStack {
                                VStack(alignment: .leading, spacing: 4) {
                                    Text("Daemon")
                                        .foregroundStyle(MobilePalette.textSecondary)
                                    Text(monitoring.system.daemon.running ? "Running" : "Stopped")
                                        .foregroundStyle(monitoring.system.daemon.running ? MobilePalette.green : MobilePalette.red)
                                        .font(.headline)
                                }
                                Spacer()
                                Text(monitoring.system.daemon.status.capitalized)
                                    .foregroundStyle(MobilePalette.textPrimary)
                                    .font(.subheadline.weight(.semibold))
                            }

                            ForEach(monitoring.system.sources) { source in
                                HStack {
                                    VStack(alignment: .leading, spacing: 2) {
                                        Text(source.name)
                                            .foregroundStyle(MobilePalette.textPrimary)
                                        Text("\(source.records) records • \(source.freshness)")
                                            .foregroundStyle(MobilePalette.textSecondary)
                                            .font(.caption)
                                    }
                                    Spacer()
                                    StatusPill(text: source.status)
                                }
                            }
                        }
                    }

                    if !monitoring.news.isEmpty {
                        sectionTitle("News")
                        card {
                            VStack(alignment: .leading, spacing: 14) {
                                ForEach(monitoring.news) { item in
                                    VStack(alignment: .leading, spacing: 6) {
                                        Text(item.title)
                                            .foregroundStyle(MobilePalette.textPrimary)
                                            .font(.subheadline.weight(.semibold))
                                        Text("\(item.source) • \(shortTimestamp(item.publishedAt))")
                                            .foregroundStyle(MobilePalette.textSecondary)
                                            .font(.caption)
                                    }
                                }
                            }
                        }
                    }
                }
            }
            .padding(16)
        }
        .background(MobilePalette.bgPrimary)
        .navigationTitle("Monitor")
    }

    @ViewBuilder
    private func topBar(maskValues: Binding<Bool>) -> some View {
        HStack {
            Button {
                maskValues.wrappedValue.toggle()
            } label: {
                Image(systemName: maskValues.wrappedValue ? "eye.slash.fill" : "eye.fill")
            }
            .buttonStyle(SecondaryIconButtonStyle())

            Spacer()

            Button(store.isBusy ? "Refreshing…" : "Refresh") {
                Task { await store.refresh() }
            }
            .buttonStyle(PrimaryButtonStyle())
            .frame(width: 150)
        }
    }

    @ViewBuilder
    private func signalSummaryCard(monitoring: MonitoringPayload) -> some View {
        card {
            VStack(alignment: .leading, spacing: 14) {
                Text("Signal Summary")
                    .foregroundStyle(MobilePalette.textSecondary)
                    .font(.subheadline.weight(.medium))

                HStack(spacing: 12) {
                    metricChip(label: "Technical", value: "\(monitoring.technicalSignalCount)")
                    metricChip(label: "Triggered Alerts", value: "\(monitoring.triggeredAlertCount)")
                }

                if let latest = monitoring.latestTimeframeSignal {
                    VStack(alignment: .leading, spacing: 6) {
                        HStack {
                            Text(prettySignal(latest.signalType))
                                .foregroundStyle(MobilePalette.textPrimary)
                                .font(.headline)
                            Spacer()
                            StatusPill(text: latest.severity)
                        }
                        Text(latest.description)
                            .foregroundStyle(MobilePalette.textSecondary)
                            .font(.subheadline)
                    }
                } else {
                    Text("No cross-timeframe signal is currently stored.")
                        .foregroundStyle(MobilePalette.textSecondary)
                        .font(.subheadline)
                }
            }
        }
    }

    private func masked(_ value: String?) -> String {
        guard !maskValues else { return "••••" }
        return value ?? "—"
    }
}

struct PortfolioView: View {
    @EnvironmentObject private var store: MobileStore
    @AppStorage("pftui.mobile.maskValues") private var maskValues = false

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                heroCard(
                    title: masked(store.portfolio?.totalValue?.raw),
                    subtitle: "Portfolio Value",
                    detail: store.portfolio?.dailyChangePct.map { "24H \($0.raw)" } ?? "No daily change"
                )

                ForEach(store.portfolio?.positions ?? []) { position in
                    card {
                        VStack(alignment: .leading, spacing: 10) {
                            HStack {
                                VStack(alignment: .leading, spacing: 2) {
                                    Text(position.symbol)
                                        .font(.headline)
                                        .foregroundStyle(MobilePalette.textPrimary)
                                    Text(position.name)
                                        .font(.subheadline)
                                        .foregroundStyle(MobilePalette.textSecondary)
                                }
                                Spacer()
                                Text(position.dayChangePct?.raw ?? "—")
                                    .foregroundStyle(deltaColor(position.dayChangePct?.raw))
                                    .font(.subheadline.weight(.semibold))
                            }
                            metricRow("Category", position.category)
                            metricRow("Price", masked(position.currentPrice?.raw))
                            metricRow("Value", masked(position.currentValue?.raw))
                            metricRow("Allocation", position.allocationPct?.raw ?? "—")
                        }
                    }
                }
            }
            .padding(16)
        }
        .background(MobilePalette.bgPrimary)
        .navigationTitle("Portfolio")
    }

    private func masked(_ value: String?) -> String {
        guard !maskValues else { return "••••" }
        return value ?? "—"
    }
}

struct AnalyticsView: View {
    @EnvironmentObject private var store: MobileStore

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                if let analytics = store.analytics {
                    analyticsHero(analytics: analytics)
                    analyticsSummary(analytics: analytics)

                    if let regime = analytics.regime {
                        sectionTitle("Regime Drivers")
                        card {
                            VStack(alignment: .leading, spacing: 14) {
                                if !regime.drivers.isEmpty {
                                    ScrollView(.horizontal, showsIndicators: false) {
                                        HStack(spacing: 8) {
                                            ForEach(regime.drivers, id: \.self) { driver in
                                                Text(driver)
                                                    .font(.caption.weight(.medium))
                                                    .foregroundStyle(MobilePalette.textPrimary)
                                                    .padding(.horizontal, 10)
                                                    .padding(.vertical, 8)
                                                    .background(MobilePalette.bgPrimary.opacity(0.65))
                                                    .clipShape(Capsule())
                                            }
                                        }
                                    }
                                }

                                LazyVGrid(columns: [GridItem(.flexible()), GridItem(.flexible())], spacing: 12) {
                                    analyticsStat(label: "VIX", value: formatNumber(regime.vix))
                                    analyticsStat(label: "DXY", value: formatNumber(regime.dxy))
                                    analyticsStat(label: "10Y", value: formatNumber(regime.yield10y))
                                    analyticsStat(label: "Oil", value: formatNumber(regime.oil))
                                    analyticsStat(label: "Gold", value: formatNumber(regime.gold))
                                    analyticsStat(label: "BTC", value: formatNumber(regime.btc))
                                }
                            }
                        }
                    }

                    if !analytics.sentiment.isEmpty {
                        sectionTitle("Sentiment")
                        card {
                            VStack(spacing: 14) {
                                ForEach(analytics.sentiment) { item in
                                    SentimentMeter(item: item)
                                }
                            }
                        }
                    }

                    if !analytics.correlations.isEmpty {
                        sectionTitle("Correlations")
                        card {
                            VStack(spacing: 12) {
                                ForEach(analytics.correlations) { item in
                                    compactRow(
                                        title: "\(item.symbolA) / \(item.symbolB)",
                                        subtitle: "\(item.period) • \(shortTimestamp(item.recordedAt))",
                                        trailing: String(format: "%.2f", item.correlation),
                                        trailingColor: correlationColor(item.correlation)
                                    )
                                }
                            }
                        }
                    }

                    if !analytics.predictions.isEmpty {
                        sectionTitle("Prediction Markets")
                        card {
                            VStack(alignment: .leading, spacing: 14) {
                                ForEach(analytics.predictions) { item in
                                    VStack(alignment: .leading, spacing: 6) {
                                        HStack(alignment: .top) {
                                            Text(item.question)
                                                .foregroundStyle(MobilePalette.textPrimary)
                                                .font(.subheadline.weight(.semibold))
                                            Spacer()
                                            Text(String(format: "%.0f%%", item.probabilityPct))
                                                .foregroundStyle(MobilePalette.accent)
                                                .font(.subheadline.weight(.bold))
                                        }
                                        Text(item.category.capitalized)
                                            .foregroundStyle(MobilePalette.textSecondary)
                                            .font(.caption)
                                    }
                                }
                            }
                        }
                    }
                }

                sectionTitle("Timeframe Scores")
                ForEach(store.analytics?.timeframes ?? []) { timeframe in
                    card {
                        VStack(alignment: .leading, spacing: 12) {
                            HStack(alignment: .firstTextBaseline) {
                                Text(timeframe.label)
                                    .font(.headline)
                                    .foregroundStyle(MobilePalette.textPrimary)
                                Spacer()
                                Text(String(format: "%.0f", timeframe.score))
                                    .font(.title3.weight(.bold))
                                    .foregroundStyle(scoreColor(timeframe.score))
                            }

                            ScoreBar(score: timeframe.score)

                            Text(timeframe.summary ?? "No score set yet.")
                                .font(.subheadline)
                                .foregroundStyle(MobilePalette.textSecondary)

                            if let updatedAt = timeframe.updatedAt {
                                Text("Updated \(shortTimestamp(updatedAt))")
                                    .font(.caption)
                                    .foregroundStyle(MobilePalette.textSecondary)
                            }
                        }
                    }
                }
            }
            .padding(16)
        }
        .background(MobilePalette.bgPrimary)
        .navigationTitle("Analytics")
        .toolbar {
            ToolbarItem(placement: .primaryAction) {
                Button("Disconnect") {
                    store.disconnect()
                }
                .foregroundStyle(MobilePalette.red)
            }
        }
    }

    @ViewBuilder
    private func analyticsHero(analytics: AnalyticsPayload) -> some View {
        let title = analytics.regime?.regime.replacingOccurrences(of: "_", with: " ").capitalized ?? "No Regime"
        let detail = analytics.regime?.confidence.map { "Confidence \(Int($0 * 100))%" } ?? "\(analytics.timeframes.count) timeframes tracked"

        heroCard(title: title, subtitle: "Analytics Engine", detail: detail)
    }

    @ViewBuilder
    private func analyticsSummary(analytics: AnalyticsPayload) -> some View {
        let avg = averageScore(analytics.timeframes)
        let strongest = strongestTimeframe(analytics.timeframes)

        card {
            VStack(alignment: .leading, spacing: 14) {
                Text("Overview")
                    .foregroundStyle(MobilePalette.textSecondary)
                    .font(.subheadline.weight(.medium))

                LazyVGrid(columns: [GridItem(.flexible()), GridItem(.flexible())], spacing: 12) {
                    analyticsStat(label: "Average Score", value: String(format: "%.0f", avg))
                    analyticsStat(label: "Strongest Layer", value: strongest?.label ?? "—")
                    analyticsStat(label: "Correlations", value: "\(analytics.correlations.count)")
                    analyticsStat(label: "Prediction Signals", value: "\(analytics.predictions.count)")
                }
            }
        }
    }
}

struct ScoreBar: View {
    let score: Double

    var body: some View {
        GeometryReader { proxy in
            let width = max(proxy.size.width, 1)
            let normalized = CGFloat((score + 100.0) / 200.0).clamped(to: 0...1)

            ZStack(alignment: .leading) {
                RoundedRectangle(cornerRadius: 10)
                    .fill(
                        LinearGradient(
                            colors: [MobilePalette.red, MobilePalette.amber, MobilePalette.green],
                            startPoint: .leading,
                            endPoint: .trailing
                        )
                    )
                    .frame(height: 14)

                Capsule()
                    .fill(Color.white.opacity(0.9))
                    .frame(width: 3, height: 24)
                    .offset(x: width * normalized - 1.5)
            }
        }
        .frame(height: 24)
    }
}

struct AppField: View {
    let title: String
    @Binding var text: String
    let placeholder: String
    var keyboard: UIKeyboardType = .default
    var secure = false

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text(title)
                .foregroundStyle(MobilePalette.textSecondary)
                .font(.caption)
            if secure {
                SecureField(placeholder, text: $text)
                    .padding(14)
                    .background(MobilePalette.bgSecondary)
                    .overlay(RoundedRectangle(cornerRadius: 14).stroke(MobilePalette.border))
                    .clipShape(RoundedRectangle(cornerRadius: 14))
                    .foregroundStyle(MobilePalette.textPrimary)
            } else {
                TextField(placeholder, text: $text)
#if canImport(UIKit)
                    .keyboardType(keyboard)
                    .textInputAutocapitalization(.never)
                    .autocorrectionDisabled()
#endif
                    .padding(14)
                    .background(MobilePalette.bgSecondary)
                    .overlay(RoundedRectangle(cornerRadius: 14).stroke(MobilePalette.border))
                    .clipShape(RoundedRectangle(cornerRadius: 14))
                    .foregroundStyle(MobilePalette.textPrimary)
            }
        }
    }
}

struct PrimaryButtonStyle: ButtonStyle {
    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .padding(.vertical, 14)
            .background(
                LinearGradient(
                    colors: [
                        MobilePalette.green.opacity(configuration.isPressed ? 0.72 : 1),
                        MobilePalette.accent.opacity(configuration.isPressed ? 0.72 : 1),
                        MobilePalette.blue.opacity(configuration.isPressed ? 0.72 : 1)
                    ],
                    startPoint: .leading,
                    endPoint: .trailing
                )
            )
            .foregroundStyle(Color.black)
            .font(.headline)
            .clipShape(RoundedRectangle(cornerRadius: 14))
    }
}

struct SecondaryIconButtonStyle: ButtonStyle {
    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .foregroundStyle(MobilePalette.textPrimary)
            .padding(10)
            .background(MobilePalette.bgSecondary)
            .overlay(RoundedRectangle(cornerRadius: 12).stroke(MobilePalette.border))
            .clipShape(RoundedRectangle(cornerRadius: 12))
            .opacity(configuration.isPressed ? 0.8 : 1.0)
    }
}

struct StatusPill: View {
    let text: String

    var body: some View {
        Text(text.capitalized)
            .font(.caption.bold())
            .padding(.horizontal, 10)
            .padding(.vertical, 6)
            .foregroundStyle(color)
            .background(color.opacity(0.16))
            .clipShape(Capsule())
    }

    private var color: Color {
        switch text.lowercased() {
        case "fresh", "running", "notable":
            return MobilePalette.green
        case "stale", "warning":
            return MobilePalette.amber
        case "critical", "stopped", "empty":
            return MobilePalette.red
        default:
            return MobilePalette.accent
        }
    }
}

struct SentimentMeter: View {
    let item: SentimentPayload

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack {
                VStack(alignment: .leading, spacing: 2) {
                    Text(item.indexType.capitalized)
                        .foregroundStyle(MobilePalette.textPrimary)
                        .font(.subheadline.weight(.semibold))
                    Text(item.classification.capitalized)
                        .foregroundStyle(MobilePalette.textSecondary)
                        .font(.caption)
                }
                Spacer()
                Text("\(item.value)")
                    .foregroundStyle(scoreColor(Double(item.value) - 50))
                    .font(.title3.weight(.bold))
            }

            GeometryReader { proxy in
                let width = max(proxy.size.width, 1)
                let fill = width * CGFloat(Double(item.value) / 100.0)
                ZStack(alignment: .leading) {
                    Capsule()
                        .fill(MobilePalette.bgPrimary.opacity(0.7))
                    Capsule()
                        .fill(
                            LinearGradient(
                                colors: [MobilePalette.red, MobilePalette.amber, MobilePalette.green],
                                startPoint: .leading,
                                endPoint: .trailing
                            )
                        )
                        .frame(width: fill)
                }
            }
            .frame(height: 10)
        }
    }
}

@ViewBuilder
private func card<Content: View>(@ViewBuilder _ content: () -> Content) -> some View {
    content()
        .padding(16)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(
            LinearGradient(
                colors: [MobilePalette.bgSecondary, MobilePalette.bgTertiary],
                startPoint: .topLeading,
                endPoint: .bottomTrailing
            )
        )
        .overlay(RoundedRectangle(cornerRadius: 20).stroke(MobilePalette.border))
        .clipShape(RoundedRectangle(cornerRadius: 20))
}

@ViewBuilder
private func heroCard(title: String, subtitle: String, detail: String) -> some View {
    ZStack(alignment: .bottomLeading) {
        RoundedRectangle(cornerRadius: 24)
            .fill(
                LinearGradient(
                    colors: [
                        MobilePalette.green.opacity(0.22),
                        MobilePalette.accent.opacity(0.2),
                        MobilePalette.blue.opacity(0.18),
                        MobilePalette.bgPrimary
                    ],
                    startPoint: .topLeading,
                    endPoint: .bottomTrailing
                )
            )
            .overlay(RoundedRectangle(cornerRadius: 24).stroke(MobilePalette.border))

        VStack(alignment: .leading, spacing: 10) {
            Text(subtitle.uppercased())
                .font(.caption.weight(.semibold))
                .foregroundStyle(MobilePalette.textSecondary)
            Text(title)
                .font(.system(size: 34, weight: .bold, design: .rounded))
                .foregroundStyle(MobilePalette.textPrimary)
            Text(detail)
                .foregroundStyle(MobilePalette.accent)
                .font(.subheadline.weight(.medium))
        }
        .padding(20)
    }
    .frame(height: 170)
}

@ViewBuilder
private func metricChip(label: String, value: String) -> some View {
    VStack(alignment: .leading, spacing: 4) {
        Text(label)
            .foregroundStyle(MobilePalette.textSecondary)
            .font(.caption)
        Text(value)
            .foregroundStyle(MobilePalette.textPrimary)
            .font(.title3.weight(.bold))
            .lineLimit(2)
    }
    .padding(12)
    .frame(maxWidth: .infinity, alignment: .leading)
    .background(MobilePalette.bgPrimary.opacity(0.45))
    .clipShape(RoundedRectangle(cornerRadius: 16))
}

@ViewBuilder
private func sectionTitle(_ text: String) -> some View {
    Text(text)
        .foregroundStyle(MobilePalette.textPrimary)
        .font(.headline)
}

@ViewBuilder
private func compactRow(title: String, subtitle: String, trailing: String, trailingColor: Color) -> some View {
    HStack(alignment: .top) {
        VStack(alignment: .leading, spacing: 4) {
            Text(title)
                .foregroundStyle(MobilePalette.textPrimary)
                .font(.subheadline.weight(.semibold))
            Text(subtitle)
                .foregroundStyle(MobilePalette.textSecondary)
                .font(.caption)
        }
        Spacer()
        Text(trailing)
            .foregroundStyle(trailingColor)
            .font(.subheadline.weight(.semibold))
    }
}

private func metricRow(_ title: String, _ value: String) -> some View {
    HStack {
        Text(title)
            .foregroundStyle(MobilePalette.textSecondary)
        Spacer()
        Text(value)
            .foregroundStyle(MobilePalette.textPrimary)
    }
    .font(.subheadline)
}

private func scoreColor(_ score: Double) -> Color {
    if score > 15 { return MobilePalette.green }
    if score < -15 { return MobilePalette.red }
    return MobilePalette.textSecondary
}

@ViewBuilder
private func analyticsStat(label: String, value: String) -> some View {
    metricChip(label: label, value: value)
}

private func deltaColor(_ raw: String?) -> Color {
    guard let raw else { return MobilePalette.textPrimary }
    if raw.contains("-") { return MobilePalette.red }
    if raw == "—" { return MobilePalette.textSecondary }
    return MobilePalette.green
}

private func prettySignal(_ raw: String) -> String {
    raw
        .replacingOccurrences(of: "_", with: " ")
        .capitalized
}

private func shortTimestamp(_ raw: String) -> String {
    let iso = ISO8601DateFormatter()
    if let date = iso.date(from: raw) {
        let formatter = RelativeDateTimeFormatter()
        formatter.unitsStyle = .abbreviated
        return formatter.localizedString(for: date, relativeTo: Date())
    }
    return raw
}

private func averageScore(_ timeframes: [TimeframePayload]) -> Double {
    guard !timeframes.isEmpty else { return 0 }
    return timeframes.map(\.score).reduce(0, +) / Double(timeframes.count)
}

private func strongestTimeframe(_ timeframes: [TimeframePayload]) -> TimeframePayload? {
    timeframes.max { abs($0.score) < abs($1.score) }
}

private func correlationColor(_ value: Double) -> Color {
    if value >= 0.6 { return MobilePalette.green }
    if value <= -0.6 { return MobilePalette.red }
    return MobilePalette.accent
}

private func formatNumber(_ value: Double?) -> String {
    guard let value else { return "—" }
    if value >= 1000 {
        return String(format: "%.0f", value)
    }
    if value >= 100 {
        return String(format: "%.1f", value)
    }
    return String(format: "%.2f", value)
}

private extension CGFloat {
    func clamped(to range: ClosedRange<Self>) -> Self {
        Swift.min(Swift.max(self, range.lowerBound), range.upperBound)
    }
}
