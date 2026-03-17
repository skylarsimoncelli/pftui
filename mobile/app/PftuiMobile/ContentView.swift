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

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                HStack {
                    Spacer()
                    Button(store.isBusy ? "Refreshing…" : "Refresh") {
                        Task { await store.refresh() }
                    }
                    .buttonStyle(PrimaryButtonStyle())
                    .frame(width: 140)
                }
                headerCard(title: store.portfolio?.totalValue?.raw ?? "—",
                           subtitle: "Total Value",
                           delta: store.portfolio?.dailyChangePct?.raw,
                           deltaPrefix: "1D")

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
                                    Badge(text: position.gainPct?.raw ?? "—")
                                }
                                metricRow("Price", position.currentPrice?.raw ?? "—")
                                metricRow("Value", position.currentValue?.raw ?? "—")
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
}

struct AnalyticsView: View {
    @EnvironmentObject private var store: MobileStore

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                headerCard(title: store.analytics?.summary.totalValue?.raw ?? "—",
                           subtitle: "Tracked Value",
                           delta: store.analytics?.performance.metrics.totalReturnPct?.raw,
                           deltaPrefix: "Return")

                card {
                    VStack(alignment: .leading, spacing: 12) {
                        Text("Top Movers")
                            .font(.headline)
                            .foregroundStyle(MobilePalette.textPrimary)
                        ForEach(store.analytics?.summary.topMovers ?? []) { mover in
                            HStack {
                                VStack(alignment: .leading) {
                                    Text(mover.symbol)
                                        .foregroundStyle(MobilePalette.textPrimary)
                                    Text(mover.name)
                                        .font(.caption)
                                        .foregroundStyle(MobilePalette.textSecondary)
                                }
                                Spacer()
                                Badge(text: mover.gainPct?.raw ?? "—")
                            }
                        }
                    }
                }

                card {
                    VStack(alignment: .leading, spacing: 12) {
                        Text("Macro Pulse")
                            .font(.headline)
                            .foregroundStyle(MobilePalette.textPrimary)
                        ForEach(store.analytics?.macroView.topMovers ?? []) { item in
                            HStack {
                                VStack(alignment: .leading) {
                                    Text(item.name)
                                        .foregroundStyle(MobilePalette.textPrimary)
                                    Text(item.symbol)
                                        .font(.caption)
                                        .foregroundStyle(MobilePalette.textSecondary)
                                }
                                Spacer()
                                VStack(alignment: .trailing) {
                                    Text(item.value?.raw ?? "—")
                                        .foregroundStyle(MobilePalette.textPrimary)
                                    Text(item.changePct?.raw ?? "—")
                                        .font(.caption)
                                        .foregroundStyle(MobilePalette.textSecondary)
                                }
                            }
                        }
                    }
                }
            }
            .padding(16)
        }
        .background(MobilePalette.bgPrimary)
        .navigationTitle("Analytics")
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
