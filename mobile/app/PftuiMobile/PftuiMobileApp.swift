import SwiftUI

@main
struct PftuiMobileApp: App {
    @StateObject private var store = MobileStore()

    var body: some Scene {
        WindowGroup {
            RootView()
                .environmentObject(store)
                .preferredColorScheme(.dark)
        }
    }
}
