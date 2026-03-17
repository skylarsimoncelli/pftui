import SwiftUI
#if canImport(UIKit)
import UIKit
#else
typealias UIKeyboardType = Int
extension UIKeyboardType {
    static let `default` = 0
    static let numberPad = 1
}
#endif

private enum MobilePalette {
    static let bgPrimary = Color(red: 13/255, green: 17/255, blue: 23/255)
    static let bgSecondary = Color(red: 22/255, green: 27/255, blue: 34/255)
    static let bgTertiary = Color(red: 33/255, green: 38/255, blue: 45/255)
    static let border = Color(red: 48/255, green: 54/255, blue: 61/255)
    static let textPrimary = Color(red: 201/255, green: 209/255, blue: 217/255)
    static let textSecondary = Color(red: 139/255, green: 148/255, blue: 158/255)
    static let accent = Color(red: 137/255, green: 180/255, blue: 250/255)
    static let green = Color(red: 166/255, green: 227/255, blue: 161/255)
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
                DashboardView(selectedTab: $selectedTab)
                    .task {
                        if store.portfolio == nil || store.analytics == nil {
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
                VStack(alignment: .leading, spacing: 20) {
                    Text("pftui mobile")
                        .font(.largeTitle.bold())
                        .foregroundStyle(MobilePalette.textPrimary)
                    Text("Connect to the local pftui mobile API over TLS. Enter `hostname` or `hostname:port`, your generated API token, and the fingerprint printed by `pftui system mobile serve`.")
                        .foregroundStyle(MobilePalette.textSecondary)
                    VStack(spacing: 14) {
                        AppField(title: "Server", text: $server, placeholder: "192.168.1.10:9443")
                        AppField(title: "API Token", text: $apiToken, placeholder: "pftm_read_...", secure: true)
                        AppField(title: "Fingerprint", text: $fingerprint, placeholder: "AA:BB:CC:...")
                    }
                    Button {
                        Task {
                            await store.connect(server: server, apiToken: apiToken, fingerprint: fingerprint)
                        }
                    } label: {
                        Text(store.isBusy ? "Connecting…" : "Connect")
                            .frame(maxWidth: .infinity)
                    }
                    .buttonStyle(PrimaryButtonStyle())
                    if let error = store.errorMessage {
                        Text(error)
                            .foregroundStyle(MobilePalette.red)
                            .font(.footnote)
                    }
                    VStack(alignment: .leading, spacing: 8) {
                        Text("Server setup")
                            .foregroundStyle(MobilePalette.textPrimary)
                            .font(.headline)
                        Text("1. Run `pftui system mobile enable --bind 0.0.0.0` once")
                        Text("2. Run `pftui system mobile token generate --permission read --name ios`")
                        Text("3. Start `pftui system mobile serve`")
                        Text("4. Copy the printed fingerprint and token into this screen")
                    }
                    .font(.footnote)
                    .foregroundStyle(MobilePalette.textSecondary)
                    .padding()
                    .background(MobilePalette.bgSecondary)
                    .overlay(RoundedRectangle(cornerRadius: 16).stroke(MobilePalette.border))
                    .clipShape(RoundedRectangle(cornerRadius: 16))
                }
                .padding(20)
            }
        }
    }
}

struct DashboardView: View {
    @EnvironmentObject private var store: MobileStore
    @Binding var selectedTab: Int

    var body: some View {
        TabView(selection: $selectedTab) {
            NavigationStack {
                PortfolioView()
            }
            .tabItem {
                Label("Portfolio", systemImage: "briefcase.fill")
            }
            .tag(0)

            NavigationStack {
                AnalyticsView()
            }
            .tabItem {
                Label("Analytics", systemImage: "chart.xyaxis.line")
            }
            .tag(1)
        }
        .tint(MobilePalette.accent)
    }
}

struct PortfolioView: View {
    @EnvironmentObject private var store: MobileStore
    @AppStorage("pftui.mobile.maskValues") private var maskValues = false

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                HStack {
                    Button {
                        maskValues.toggle()
                    } label: {
                        Image(systemName: maskValues ? "eye.slash.fill" : "eye.fill")
                    }
                    .buttonStyle(SecondaryIconButtonStyle())

                    Spacer()
                    Button(store.isBusy ? "Reloading…" : "Reload") {
                        Task { await store.refresh() }
                    }
                    .buttonStyle(PrimaryButtonStyle())
                    .frame(width: 140)
                }
                headerCard(title: masked(store.portfolio?.totalValue?.raw),
                           subtitle: "Total Value",
                           delta: store.portfolio?.dailyChangePct?.raw,
                           deltaPrefix: "24H")

                if let positions = store.portfolio?.positions {
                    ForEach(positions) { position in
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
                                    Badge(text: position.dayChangePct?.raw ?? "—")
                                }
                                metricRow("Price", masked(position.currentPrice?.raw))
                                metricRow("Value", masked(position.currentValue?.raw))
                                metricRow("Allocation", position.allocationPct?.raw ?? "—")
                            }
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
        guard maskValues else { return value ?? "—" }
        return "••••"
    }
}

struct AnalyticsView: View {
    @EnvironmentObject private var store: MobileStore

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                ForEach(store.analytics?.timeframes ?? []) { timeframe in
                    TimeframeScoreCard(timeframe: timeframe)
                }
            }
            .padding(16)
        }
        .background(MobilePalette.bgPrimary)
        .navigationTitle("Analytics")
    }
}

struct TimeframeScoreCard: View {
    let timeframe: TimeframePayload

    var body: some View {
        card {
            VStack(alignment: .leading, spacing: 12) {
                HStack(alignment: .firstTextBaseline) {
                    Text(timeframe.label)
                        .font(.headline)
                        .foregroundStyle(MobilePalette.textPrimary)
                    Spacer()
                    Text(String(format: "%.0f", timeframe.score))
                        .font(.subheadline.weight(.semibold))
                        .foregroundStyle(scoreColor)
                }

                ScoreBar(score: timeframe.score)

                Text(timeframe.summary ?? "No score set yet.")
                    .font(.subheadline)
                    .foregroundStyle(MobilePalette.textSecondary)
            }
        }
    }

    private var scoreColor: Color {
        if timeframe.score > 15 {
            return MobilePalette.green
        }
        if timeframe.score < -15 {
            return MobilePalette.red
        }
        return MobilePalette.textSecondary
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
                            colors: [MobilePalette.red, Color.yellow, MobilePalette.green],
                            startPoint: .leading,
                            endPoint: .trailing
                        )
                    )
                    .frame(height: 14)

                Rectangle()
                    .fill(Color.white.opacity(0.95))
                    .frame(width: 2, height: 22)
                    .offset(x: width * normalized - 1)
            }
        }
        .frame(height: 22)
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
            .background(MobilePalette.accent.opacity(configuration.isPressed ? 0.75 : 1.0))
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

struct Badge: View {
    let text: String

    var body: some View {
        Text(text)
            .font(.caption.bold())
            .foregroundStyle(color)
            .padding(.horizontal, 10)
            .padding(.vertical, 6)
            .background(color.opacity(0.15))
            .clipShape(Capsule())
    }

    private var color: Color {
        text.contains("-") ? MobilePalette.red : MobilePalette.green
    }
}

@ViewBuilder
private func card<Content: View>(@ViewBuilder _ content: () -> Content) -> some View {
    content()
        .padding(16)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(MobilePalette.bgSecondary)
        .overlay(RoundedRectangle(cornerRadius: 18).stroke(MobilePalette.border))
        .clipShape(RoundedRectangle(cornerRadius: 18))
}

@ViewBuilder
private func headerCard(title: String, subtitle: String, delta: String?, deltaPrefix: String) -> some View {
    card {
        VStack(alignment: .leading, spacing: 10) {
            Text(subtitle)
                .foregroundStyle(MobilePalette.textSecondary)
                .font(.subheadline)
            Text(title)
                .font(.system(size: 32, weight: .bold, design: .rounded))
                .foregroundStyle(MobilePalette.textPrimary)
            if let delta {
                Text("\(deltaPrefix): \(delta)")
                    .foregroundStyle(delta.contains("-") ? MobilePalette.red : MobilePalette.green)
                    .font(.footnote.weight(.semibold))
            }
        }
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

private extension CGFloat {
    func clamped(to range: ClosedRange<Self>) -> Self {
        Swift.min(Swift.max(self, range.lowerBound), range.upperBound)
    }
}
