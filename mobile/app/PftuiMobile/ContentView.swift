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
                Label("Situation", systemImage: "scope")
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

            NavigationStack {
                SystemView()
            }
            .tabItem {
                Label("System", systemImage: "server.rack")
            }
            .tag(3)
        }
        .tint(MobilePalette.accent)
    }
}

struct HomeView: View {
    @EnvironmentObject private var store: MobileStore
    @AppStorage("pftui.mobile.maskValues") private var maskValues = false
    @AppStorage("pftui.mobile.homeDensity") private var homeDensity = "dense"
    @AppStorage("pftui.mobile.home.showSituation") private var showSituation = true
    @AppStorage("pftui.mobile.home.showFocus") private var showFocus = true
    @AppStorage("pftui.mobile.home.showChanges") private var showChanges = true
    @AppStorage("pftui.mobile.home.showRisk") private var showRisk = true
    @AppStorage("pftui.mobile.home.showTimeframes") private var showTimeframes = true
    @AppStorage("pftui.mobile.home.showConcentration") private var showConcentration = true
    @AppStorage("pftui.mobile.home.showPulse") private var showPulse = true
    @AppStorage("pftui.mobile.home.showWatchlist") private var showWatchlist = true
    @AppStorage("pftui.mobile.home.showSystem") private var showSystem = false
    @AppStorage("pftui.mobile.home.showNews") private var showNews = false

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 18) {
                topBar(maskValues: $maskValues)

                heroCard(
                    title: store.dashboard?.situation.title ?? situationTitle,
                    subtitle: "Situation Room",
                    detail: store.dashboard?.situation.subtitle ?? situationSubtitle
                )

                if let dashboard = store.dashboard {
                    signalSummaryCard(monitoring: dashboard.monitoring)
                    commandDeckCard(dashboard: dashboard)

                    CollapsibleCardSection(
                        title: "Watch Now",
                        subtitle: "Ranked anomalies and state shifts",
                        isExpanded: $showSituation
                    ) {
                        VStack(spacing: 12) {
                            ForEach(dashboard.situation.watchNow.prefix(homeDensity == "dense" ? 4 : 6)) { insight in
                                insightRow(insight)
                            }
                        }
                    }

                    if !dashboard.situation.portfolioImpacts.isEmpty {
                        CollapsibleCardSection(
                            title: "Portfolio Impact",
                            subtitle: "What matters to current exposure",
                            isExpanded: $showFocus
                        ) {
                            VStack(spacing: 12) {
                                ForEach(dashboard.situation.portfolioImpacts.prefix(homeDensity == "dense" ? 4 : 6)) { insight in
                                    insightRow(insight)
                                }
                            }
                        }
                    }

                    CollapsibleCardSection(
                        title: "Change Radar",
                        subtitle: "What changed since the last refresh",
                        isExpanded: $showChanges
                    ) {
                        VStack(spacing: 12) {
                            ForEach(changeRadarInsights(current: dashboard, previous: store.previousDashboard).prefix(homeDensity == "dense" ? 4 : 6)) { insight in
                                insightRow(insight)
                            }
                        }
                    }

                    if !dashboard.situation.riskMatrix.isEmpty {
                        CollapsibleCardSection(
                            title: "Risk Matrix",
                            subtitle: "Cross-asset stress map",
                            isExpanded: $showRisk
                        ) {
                            VStack(spacing: 12) {
                                ForEach(dashboard.situation.riskMatrix) { signal in
                                    riskRow(signal)
                                }
                            }
                        }
                    }

                    if let analytics = store.analytics, !analytics.timeframes.isEmpty {
                        CollapsibleCardSection(
                            title: "Timeframe Stack",
                            subtitle: homeDensity == "dense" ? "4-layer engine" : "Expanded read",
                            isExpanded: $showTimeframes
                        ) {
                            let columns = homeDensity == "dense"
                                ? [GridItem(.flexible()), GridItem(.flexible())]
                                : [GridItem(.flexible())]
                            LazyVGrid(columns: columns, spacing: 12) {
                                ForEach(analytics.timeframes) { timeframe in
                                    timeframeChip(timeframe)
                                }
                            }
                        }
                    }

                    if let positions = store.portfolio?.positions, !positions.isEmpty {
                        CollapsibleCardSection(
                            title: "Portfolio Concentration",
                            subtitle: homeDensity == "dense" ? "Top allocations" : "Extended breakdown",
                            isExpanded: $showConcentration
                        ) {
                            VStack(spacing: 12) {
                                ForEach(Array(positions.prefix(homeDensity == "dense" ? 4 : 7))) { position in
                                    holdingRow(position)
                                }
                            }
                        }
                    }

                    if !dashboard.monitoring.marketPulse.isEmpty {
                        CollapsibleCardSection(
                            title: "Market Pulse",
                            subtitle: "Cross-asset tape",
                            isExpanded: $showPulse
                        ) {
                            VStack(spacing: 12) {
                                ForEach(dashboard.monitoring.marketPulse) { item in
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

                    if !dashboard.monitoring.watchlist.isEmpty {
                        CollapsibleCardSection(
                            title: "Watchlist Pressure",
                            subtitle: "Targets and proximity",
                            isExpanded: $showWatchlist
                        ) {
                            VStack(spacing: 12) {
                                ForEach(dashboard.monitoring.watchlist.prefix(homeDensity == "dense" ? 5 : 8)) { item in
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

                    CollapsibleCardSection(
                        title: "Operational Snapshot",
                        subtitle: "Server and daemon",
                        isExpanded: $showSystem
                    ) {
                        VStack(alignment: .leading, spacing: 14) {
                            HStack(spacing: 12) {
                                metricChip(label: "Server", value: dashboard.monitoring.system.server.pftuiVersion)
                                metricChip(label: "DB", value: dashboard.monitoring.system.server.backend.uppercased())
                            }

                            HStack(spacing: 12) {
                                metricChip(label: "Daemon", value: dashboard.monitoring.system.daemon.running ? "Running" : "Stopped")
                                metricChip(label: "Stale Sources", value: "\(dashboard.monitoring.system.database.staleSources)")
                            }

                            Text("Last sync \(shortTimestamp(dashboard.generatedAt))")
                                .foregroundStyle(MobilePalette.textSecondary)
                                .font(.caption)
                        }
                    }

                    if !dashboard.monitoring.news.isEmpty {
                        CollapsibleCardSection(
                            title: "Catalysts",
                            subtitle: "Latest headlines",
                            isExpanded: $showNews
                        ) {
                            VStack(alignment: .leading, spacing: 14) {
                                ForEach(dashboard.monitoring.news.prefix(homeDensity == "dense" ? 4 : 6)) { item in
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
        .navigationTitle("Situation")
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

            Menu {
                Button(homeDensity == "dense" ? "Dense Layout ✓" : "Dense Layout") {
                    homeDensity = "dense"
                }
                Button(homeDensity == "expanded" ? "Expanded Layout ✓" : "Expanded Layout") {
                    homeDensity = "expanded"
                }
            } label: {
                Image(systemName: "slider.horizontal.3")
            }
            .buttonStyle(SecondaryIconButtonStyle())

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

    @ViewBuilder
    private func commandDeckCard(dashboard: DashboardPayload) -> some View {
        card {
            VStack(alignment: .leading, spacing: 14) {
                Text("Command Deck")
                    .foregroundStyle(MobilePalette.textSecondary)
                    .font(.subheadline.weight(.medium))

                let columns = homeDensity == "dense"
                    ? [GridItem(.flexible()), GridItem(.flexible())]
                    : [GridItem(.flexible()), GridItem(.flexible()), GridItem(.flexible())]

                LazyVGrid(columns: columns, spacing: 12) {
                    ForEach(dashboard.situation.summary) { item in
                        metricChip(label: item.label, value: item.value)
                    }
                    if homeDensity != "dense" {
                        metricChip(label: "Backend", value: dashboard.monitoring.system.server.backend.uppercased())
                        metricChip(label: "Version", value: dashboard.monitoring.system.server.pftuiVersion)
                    }
                }
            }
        }
    }

    @ViewBuilder
    private func insightRow(_ insight: SituationInsightPayload) -> some View {
        HStack(alignment: .top, spacing: 12) {
            Circle()
                .fill(insightColor(insight.severity))
                .frame(width: 10, height: 10)
                .padding(.top, 6)

            VStack(alignment: .leading, spacing: 5) {
                HStack(alignment: .top) {
                    Text(insight.title)
                        .foregroundStyle(MobilePalette.textPrimary)
                        .font(.subheadline.weight(.semibold))
                    Spacer()
                    Text(insight.value)
                        .foregroundStyle(insightColor(insight.severity))
                        .font(.caption.weight(.bold))
                }

                Text(insight.detail)
                    .foregroundStyle(MobilePalette.textSecondary)
                    .font(.caption)
            }
        }
        .padding(12)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(MobilePalette.bgPrimary.opacity(0.45))
        .clipShape(RoundedRectangle(cornerRadius: 16))
    }

    @ViewBuilder
    private func riskRow(_ signal: RiskSignalPayload) -> some View {
        HStack(spacing: 12) {
            VStack(alignment: .leading, spacing: 4) {
                Text(signal.label)
                    .foregroundStyle(MobilePalette.textPrimary)
                    .font(.subheadline.weight(.semibold))
                Text(signal.detail)
                    .foregroundStyle(MobilePalette.textSecondary)
                    .font(.caption)
            }
            Spacer()
            VStack(alignment: .trailing, spacing: 4) {
                Text(signal.value)
                    .foregroundStyle(insightColor(signal.severity))
                    .font(.subheadline.weight(.bold))
                StatusPill(text: signal.status)
            }
        }
        .padding(12)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(MobilePalette.bgPrimary.opacity(0.45))
        .clipShape(RoundedRectangle(cornerRadius: 16))
    }

    @ViewBuilder
    private func timeframeChip(_ timeframe: TimeframePayload) -> some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack {
                Text(timeframe.label)
                    .foregroundStyle(MobilePalette.textPrimary)
                    .font(.subheadline.weight(.semibold))
                    .lineLimit(1)
                Spacer()
                Text(String(format: "%.0f", timeframe.score))
                    .foregroundStyle(scoreColor(timeframe.score))
                    .font(.subheadline.weight(.bold))
            }

            ScoreBar(score: timeframe.score)
                .frame(height: 18)

            if homeDensity != "dense" {
                Text(timeframe.summary ?? "No narrative yet.")
                    .foregroundStyle(MobilePalette.textSecondary)
                    .font(.caption)
                    .lineLimit(2)
            }
        }
        .padding(12)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(MobilePalette.bgPrimary.opacity(0.45))
        .clipShape(RoundedRectangle(cornerRadius: 16))
    }

    @ViewBuilder
    private func holdingRow(_ position: PositionPayload) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack {
                VStack(alignment: .leading, spacing: 2) {
                    Text(position.symbol)
                        .foregroundStyle(MobilePalette.textPrimary)
                        .font(.subheadline.weight(.semibold))
                    Text(position.name)
                        .foregroundStyle(MobilePalette.textSecondary)
                        .font(.caption)
                        .lineLimit(1)
                }
                Spacer()
                Text(masked(position.currentValue?.raw))
                    .foregroundStyle(MobilePalette.textPrimary)
                    .font(.subheadline.weight(.semibold))
            }

            let allocation = percentageValue(position.allocationPct?.raw)
            ZStack(alignment: .leading) {
                Capsule()
                    .fill(MobilePalette.bgPrimary.opacity(0.7))
                    .frame(height: 8)
                Capsule()
                    .fill(
                        LinearGradient(
                            colors: [MobilePalette.green, MobilePalette.accent, MobilePalette.blue],
                            startPoint: .leading,
                            endPoint: .trailing
                        )
                    )
                    .frame(width: CGFloat(allocation).clamped(to: 0...100) * 2.4, height: 8)
            }

            HStack {
                Text(position.allocationPct?.raw ?? "—")
                    .foregroundStyle(MobilePalette.accent)
                    .font(.caption.weight(.medium))
                Spacer()
                Text(position.dayChangePct?.raw ?? "—")
                    .foregroundStyle(deltaColor(position.dayChangePct?.raw))
                    .font(.caption.weight(.medium))
            }
        }
    }

    private func masked(_ value: String?) -> String {
        guard !maskValues else { return "••••" }
        return value ?? "—"
    }

    private var situationTitle: String {
        if let latest = store.dashboard?.monitoring.latestTimeframeSignal {
            return prettySignal(latest.signalType)
        }
        if let regime = store.analytics?.regime?.regime {
            return regime.replacingOccurrences(of: "_", with: " ").capitalized
        }
        return masked(store.portfolio?.totalValue?.raw)
    }

    private var situationSubtitle: String {
        if let latest = store.dashboard?.monitoring.latestTimeframeSignal {
            return latest.description
        }
        return "\(store.portfolio?.positionCount ?? 0) positions • \(store.dashboard?.monitoring.system.server.pftuiVersion ?? "offline")"
    }

    private func changeRadarInsights(current: DashboardPayload, previous: DashboardPayload?) -> [SituationInsightPayload] {
        guard let previous else {
            return [
                SituationInsightPayload(
                    title: "Baseline forming",
                    detail: "Refresh once more and the app will start ranking state changes between snapshots.",
                    value: "Warmup",
                    severity: "normal"
                )
            ]
        }

        var items: [SituationInsightPayload] = []
        let currentAverage = averageScore(current.analytics.timeframes)
        let previousAverage = averageScore(previous.analytics.timeframes)
        let averageDelta = currentAverage - previousAverage
        if abs(averageDelta) >= 8 {
            items.append(
                SituationInsightPayload(
                    title: averageDelta > 0 ? "Risk tone improved" : "Risk tone weakened",
                    detail: "Average timeframe score moved from \(Int(previousAverage)) to \(Int(currentAverage)).",
                    value: signedNumber(averageDelta),
                    severity: abs(averageDelta) >= 18 ? "critical" : "elevated"
                )
            )
        }

        let alertDelta = current.monitoring.triggeredAlertCount - previous.monitoring.triggeredAlertCount
        if alertDelta != 0 {
            items.append(
                SituationInsightPayload(
                    title: alertDelta > 0 ? "Triggered alerts increased" : "Triggered alerts cooled",
                    detail: "Alert load moved from \(previous.monitoring.triggeredAlertCount) to \(current.monitoring.triggeredAlertCount).",
                    value: signedInt(alertDelta),
                    severity: alertDelta > 0 ? "critical" : "normal"
                )
            )
        }

        let staleDelta = current.monitoring.system.database.staleSources - previous.monitoring.system.database.staleSources
        if staleDelta != 0 {
            items.append(
                SituationInsightPayload(
                    title: staleDelta > 0 ? "Data trust worsened" : "Data freshness improved",
                    detail: "Stale sources moved from \(previous.monitoring.system.database.staleSources) to \(current.monitoring.system.database.staleSources).",
                    value: signedInt(staleDelta),
                    severity: staleDelta > 0 ? "elevated" : "normal"
                )
            )
        }

        if current.analytics.regime?.regime != previous.analytics.regime?.regime,
           let regime = current.analytics.regime?.regime {
            items.append(
                SituationInsightPayload(
                    title: "Regime shifted",
                    detail: "The current market regime changed since the prior snapshot.",
                    value: regime.replacingOccurrences(of: "_", with: " ").capitalized,
                    severity: "critical"
                )
            )
        }

        let previousPulse = Dictionary(uniqueKeysWithValues: previous.monitoring.marketPulse.map { ($0.symbol, $0) })
        for item in current.monitoring.marketPulse {
            guard let prior = previousPulse[item.symbol] else { continue }
            let moveDelta = changeValue(item.dayChangePct?.raw) - changeValue(prior.dayChangePct?.raw)
            if abs(moveDelta) >= 1.5 {
                items.append(
                    SituationInsightPayload(
                        title: "\(item.symbol) momentum re-priced",
                        detail: item.name,
                        value: signedNumber(moveDelta),
                        severity: abs(moveDelta) >= 3 ? "critical" : "elevated"
                    )
                )
            }
        }

        if let currentHeadline = current.monitoring.news.first?.title,
           let previousHeadline = previous.monitoring.news.first?.title,
           currentHeadline != previousHeadline {
            items.append(
                SituationInsightPayload(
                    title: "Lead catalyst changed",
                    detail: currentHeadline,
                    value: "News",
                    severity: "normal"
                )
            )
        }

        return items.isEmpty
            ? [
                SituationInsightPayload(
                    title: "No major deltas",
                    detail: "The latest refresh did not materially change the system’s state.",
                    value: "Stable",
                    severity: "normal"
                )
            ]
            : items.sorted { severityWeight($0.severity) > severityWeight($1.severity) }
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
    @AppStorage("pftui.mobile.analyticsDensity") private var analyticsDensity = "dense"
    @State private var showOverview = true
    @State private var showRegime = true
    @State private var showSentiment = true
    @State private var showCorrelations = true
    @State private var showPredictions = true
    @State private var showScores = true

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                if let analytics = store.analytics {
                    analyticsHero(analytics: analytics)
                    CollapsibleCardSection(
                        title: "Overview",
                        subtitle: analyticsDensity == "dense" ? "High-density scan" : "Expanded read",
                        isExpanded: $showOverview
                    ) {
                        analyticsSummary(analytics: analytics)
                    }

                    if let regime = analytics.regime {
                        CollapsibleCardSection(
                            title: "Regime Drivers",
                            subtitle: shortTimestamp(regime.recordedAt),
                            isExpanded: $showRegime
                        ) {
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
                        CollapsibleCardSection(
                            title: "Sentiment",
                            subtitle: "Crypto + traditional",
                            isExpanded: $showSentiment
                        ) {
                            VStack(spacing: 14) {
                                ForEach(analytics.sentiment) { item in
                                    SentimentMeter(item: item)
                                }
                            }
                        }
                    }

                    if !analytics.correlations.isEmpty {
                        CollapsibleCardSection(
                            title: "Correlations",
                            subtitle: "Live relationship map",
                            isExpanded: $showCorrelations
                        ) {
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
                        CollapsibleCardSection(
                            title: "Prediction Markets",
                            subtitle: "Crowd-implied odds",
                            isExpanded: $showPredictions
                        ) {
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

                    CollapsibleCardSection(
                        title: "Timeframe Scores",
                        subtitle: "\(analytics.timeframes.count) tracked layers",
                        isExpanded: $showScores
                    ) {
                        VStack(spacing: 12) {
                            ForEach(analytics.timeframes) { timeframe in
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
                                        .lineLimit(analyticsDensity == "dense" ? 2 : nil)

                                    if let updatedAt = timeframe.updatedAt {
                                        Text("Updated \(shortTimestamp(updatedAt))")
                                            .font(.caption)
                                            .foregroundStyle(MobilePalette.textSecondary)
                                    }
                                }
                                .padding(12)
                                .frame(maxWidth: .infinity, alignment: .leading)
                                .background(MobilePalette.bgPrimary.opacity(0.4))
                                .clipShape(RoundedRectangle(cornerRadius: 16))
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
                Menu {
                    Button(analyticsDensity == "dense" ? "Dense Layout ✓" : "Dense Layout") {
                        analyticsDensity = "dense"
                    }
                    Button(analyticsDensity == "expanded" ? "Expanded Layout ✓" : "Expanded Layout") {
                        analyticsDensity = "expanded"
                    }
                } label: {
                    Image(systemName: "slider.horizontal.3")
                }
            }
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

        VStack(alignment: .leading, spacing: 14) {
            LazyVGrid(columns: [GridItem(.flexible()), GridItem(.flexible())], spacing: 12) {
                analyticsStat(label: "Average Score", value: String(format: "%.0f", avg))
                analyticsStat(label: "Strongest Layer", value: strongest?.label ?? "—")
                analyticsStat(label: "Correlations", value: "\(analytics.correlations.count)")
                analyticsStat(label: "Prediction Signals", value: "\(analytics.predictions.count)")
            }

            if analyticsDensity != "dense" {
                VStack(spacing: 10) {
                    ForEach(analytics.timeframes) { timeframe in
                        compactRow(
                            title: timeframe.label,
                            subtitle: timeframe.summary ?? "No summary",
                            trailing: String(format: "%.0f", timeframe.score),
                            trailingColor: scoreColor(timeframe.score)
                        )
                    }
                }
            }
        }
    }
}

struct SystemView: View {
    @EnvironmentObject private var store: MobileStore
    @AppStorage("pftui.mobile.homeDensity") private var homeDensity = "dense"
    @AppStorage("pftui.mobile.analyticsDensity") private var analyticsDensity = "dense"
    @AppStorage("pftui.mobile.systemDensity") private var systemDensity = "dense"
    @AppStorage("pftui.mobile.home.showSituation") private var showSituationModule = true
    @AppStorage("pftui.mobile.home.showFocus") private var showFocusModule = true
    @AppStorage("pftui.mobile.home.showChanges") private var showChangeModule = true
    @AppStorage("pftui.mobile.home.showRisk") private var showRiskModule = true
    @AppStorage("pftui.mobile.home.showNews") private var showNewsModule = false
    @State private var showDisplay = true
    @State private var showConnection = true
    @State private var showServer = true
    @State private var showDatabase = true
    @State private var showDaemon = true
    @State private var showSources = true

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                heroCard(
                    title: store.dashboard?.monitoring.system.server.pftuiVersion ?? "Offline",
                    subtitle: "Server Control",
                    detail: store.connection?.server ?? "No active connection"
                )

                CollapsibleCardSection(
                    title: "Display Profiles",
                    subtitle: "Customize information density",
                    isExpanded: $showDisplay
                ) {
                    VStack(alignment: .leading, spacing: 14) {
                        densityPicker(title: "Home", selection: $homeDensity)
                        densityPicker(title: "Analytics", selection: $analyticsDensity)
                        densityPicker(title: "System", selection: $systemDensity)
                        Toggle("Situation brief", isOn: $showSituationModule)
                            .tint(MobilePalette.accent)
                        Toggle("Portfolio impact", isOn: $showFocusModule)
                            .tint(MobilePalette.accent)
                        Toggle("Change radar", isOn: $showChangeModule)
                            .tint(MobilePalette.accent)
                        Toggle("Risk matrix", isOn: $showRiskModule)
                            .tint(MobilePalette.accent)
                        Toggle("Catalyst/news module", isOn: $showNewsModule)
                            .tint(MobilePalette.accent)
                    }
                }

                CollapsibleCardSection(
                    title: "Connection",
                    subtitle: store.errorMessage == nil ? "Pinned TLS session" : "Attention needed",
                    isExpanded: $showConnection
                ) {
                    VStack(spacing: 12) {
                        systemMetricRow("Host", store.connection?.server ?? "—")
                        systemMetricRow("Status", connectionStatus)
                        systemMetricRow("Fingerprint", abbreviatedFingerprint(store.connection?.fingerprint))
                        systemMetricRow("Last Sync", store.dashboard.map { shortTimestamp($0.generatedAt) } ?? "—")
                    }
                }

                if let system = store.dashboard?.monitoring.system {
                    CollapsibleCardSection(
                        title: "Server Runtime",
                        subtitle: "\(system.server.backend.uppercased()) • \(system.server.databaseMode)",
                        isExpanded: $showServer
                    ) {
                        let columns = systemDensity == "dense"
                            ? [GridItem(.flexible()), GridItem(.flexible())]
                            : [GridItem(.flexible()), GridItem(.flexible()), GridItem(.flexible())]
                        LazyVGrid(columns: columns, spacing: 12) {
                            metricChip(label: "Version", value: system.server.pftuiVersion)
                            metricChip(label: "Backend", value: system.server.backend.uppercased())
                            metricChip(label: "Portfolio", value: system.server.portfolioMode.capitalized)
                            metricChip(label: "DB Mode", value: system.server.databaseMode.capitalized)
                            metricChip(label: "Port", value: "\(system.server.mobilePort)")
                            metricChip(label: "Tokens", value: "\(system.server.apiTokenCount)")
                        }
                    }

                    CollapsibleCardSection(
                        title: "Database Health",
                        subtitle: system.database.label,
                        isExpanded: $showDatabase
                    ) {
                        VStack(alignment: .leading, spacing: 14) {
                            HStack {
                                StatusPill(text: system.database.status)
                                Spacer()
                                Text(system.database.integrity.capitalized)
                                    .foregroundStyle(system.database.integrity == "ok" || system.database.integrity == "connected" ? MobilePalette.green : MobilePalette.red)
                                    .font(.subheadline.weight(.semibold))
                            }

                            LazyVGrid(columns: [GridItem(.flexible()), GridItem(.flexible())], spacing: 12) {
                                metricChip(label: "Positions", value: "\(system.database.positions)")
                                metricChip(label: "Transactions", value: "\(system.database.transactions)")
                                metricChip(label: "Watchlist", value: "\(system.database.watchlist)")
                                metricChip(label: "Tracked Prices", value: "\(system.database.trackedPrices)")
                                metricChip(label: "Stale Sources", value: "\(system.database.staleSources)")
                                metricChip(label: "Market Sync", value: system.database.lastMarketSync.map(shortTimestamp) ?? "Never")
                            }

                            if systemDensity != "dense" {
                                systemMetricRow("News Sync", system.database.lastNewsSync.map(shortTimestamp) ?? "Never")
                            }
                        }
                    }

                    CollapsibleCardSection(
                        title: "Daemon",
                        subtitle: system.daemon.running ? "Automation online" : "Automation offline",
                        isExpanded: $showDaemon
                    ) {
                        VStack(alignment: .leading, spacing: 14) {
                            HStack(spacing: 12) {
                                metricChip(label: "State", value: system.daemon.running ? "Running" : "Stopped")
                                metricChip(label: "Cycle", value: "\(system.daemon.cycle)")
                                metricChip(label: "Wake", value: "\(system.daemon.intervalSecs)s")
                            }

                            HStack(spacing: 12) {
                                metricChip(label: "Tasks", value: "\(system.daemon.taskCount)")
                                metricChip(label: "Errors", value: "\(system.daemon.errorCount)")
                                metricChip(label: "Last Beat", value: system.daemon.lastHeartbeat.map(shortTimestamp) ?? "Never")
                            }

                            if !system.daemon.tasks.isEmpty && systemDensity != "dense" {
                                FlowTagList(items: system.daemon.tasks)
                            }
                        }
                    }

                    CollapsibleCardSection(
                        title: "Source Freshness",
                        subtitle: "Data pipeline health",
                        isExpanded: $showSources
                    ) {
                        VStack(spacing: 12) {
                            ForEach(system.sources) { source in
                                HStack {
                                    VStack(alignment: .leading, spacing: 3) {
                                        Text(source.name)
                                            .foregroundStyle(MobilePalette.textPrimary)
                                            .font(.subheadline.weight(.semibold))
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
                }
            }
            .padding(16)
        }
        .background(MobilePalette.bgPrimary)
        .navigationTitle("System")
    }

    private var connectionStatus: String {
        if store.dashboard != nil && store.errorMessage == nil {
            return "Connected"
        }
        if let error = store.errorMessage, !error.isEmpty {
            return "Error"
        }
        return "Idle"
    }

    @ViewBuilder
    private func densityPicker(title: String, selection: Binding<String>) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            Text(title)
                .foregroundStyle(MobilePalette.textSecondary)
                .font(.caption)
            Picker(title, selection: selection) {
                Text("Dense").tag("dense")
                Text("Expanded").tag("expanded")
            }
            .pickerStyle(.segmented)
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

struct CollapsibleCardSection<Content: View>: View {
    let title: String
    let subtitle: String
    @Binding var isExpanded: Bool
    @ViewBuilder let content: Content

    init(
        title: String,
        subtitle: String,
        isExpanded: Binding<Bool>,
        @ViewBuilder content: () -> Content
    ) {
        self.title = title
        self.subtitle = subtitle
        self._isExpanded = isExpanded
        self.content = content()
    }

    var body: some View {
        card {
            VStack(alignment: .leading, spacing: 14) {
                Button {
                    withAnimation(.easeInOut(duration: 0.2)) {
                        isExpanded.toggle()
                    }
                } label: {
                    HStack {
                        VStack(alignment: .leading, spacing: 4) {
                            Text(title)
                                .foregroundStyle(MobilePalette.textPrimary)
                                .font(.headline)
                            Text(subtitle)
                                .foregroundStyle(MobilePalette.textSecondary)
                                .font(.caption)
                        }
                        Spacer()
                        Image(systemName: isExpanded ? "chevron.up" : "chevron.down")
                            .foregroundStyle(MobilePalette.accent)
                            .font(.caption.weight(.bold))
                    }
                }
                .buttonStyle(.plain)

                if isExpanded {
                    content
                }
            }
        }
    }
}

struct FlowTagList: View {
    let items: [String]

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            ForEach(chunked(items, by: 3), id: \.self) { row in
                HStack(spacing: 8) {
                    ForEach(row, id: \.self) { item in
                        Text(item)
                            .font(.caption.weight(.medium))
                            .foregroundStyle(MobilePalette.textPrimary)
                            .padding(.horizontal, 10)
                            .padding(.vertical, 8)
                            .background(MobilePalette.bgPrimary.opacity(0.65))
                            .clipShape(Capsule())
                    }
                    Spacer(minLength: 0)
                }
            }
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

private func systemMetricRow(_ title: String, _ value: String) -> some View {
    HStack(alignment: .top) {
        Text(title)
            .foregroundStyle(MobilePalette.textSecondary)
        Spacer()
        Text(value)
            .foregroundStyle(MobilePalette.textPrimary)
            .multilineTextAlignment(.trailing)
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

private func insightColor(_ severity: String) -> Color {
    switch severity.lowercased() {
    case "normal":
        return MobilePalette.accent
    case "elevated", "warning":
        return MobilePalette.amber
    case "critical":
        return MobilePalette.red
    default:
        return MobilePalette.accent
    }
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

private func changeValue(_ raw: String?) -> Double {
    guard let raw else { return 0 }
    let cleaned = raw.replacingOccurrences(of: "%", with: "")
        .replacingOccurrences(of: "+", with: "")
        .trimmingCharacters(in: .whitespaces)
    return Double(cleaned) ?? 0
}

private func signedInt(_ value: Int) -> String {
    value > 0 ? "+\(value)" : "\(value)"
}

private func signedNumber(_ value: Double) -> String {
    value > 0 ? String(format: "+%.0f", value) : String(format: "%.0f", value)
}

private func severityWeight(_ raw: String) -> Int {
    switch raw.lowercased() {
    case "critical":
        return 3
    case "warning", "notable", "elevated":
        return 2
    default:
        return 1
    }
}

private func percentageValue(_ raw: String?) -> Double {
    guard let raw else { return 0 }
    let cleaned = raw.replacingOccurrences(of: "%", with: "")
    return Double(cleaned) ?? 0
}

private func abbreviatedFingerprint(_ raw: String?) -> String {
    guard let raw, !raw.isEmpty else { return "—" }
    let compact = raw.replacingOccurrences(of: ":", with: "")
    guard compact.count > 12 else { return raw }
    return "\(compact.prefix(6))…\(compact.suffix(6))"
}

private func chunked(_ items: [String], by size: Int) -> [[String]] {
    stride(from: 0, to: items.count, by: size).map { index in
        Array(items[index..<Swift.min(index + size, items.count)])
    }
}

private extension CGFloat {
    func clamped(to range: ClosedRange<Self>) -> Self {
        Swift.min(Swift.max(self, range.lowerBound), range.upperBound)
    }
}
