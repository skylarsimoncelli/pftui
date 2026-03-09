class Pftui < Formula
  desc "A beautiful terminal portfolio tracker with real-time prices, charts, and market data"
  homepage "https://github.com/skylarsimoncelli/pftui"
  url "https://github.com/skylarsimoncelli/pftui/archive/refs/tags/v0.6.0.tar.gz"
  sha256 "0019dfc4b32d63c1392aa264aed2253c1e0c2fb09216f8e2cc269bbfb8bb49b5"
  license "MIT"

  depends_on "rust" => :build

  def install
    system "cargo", "install", *std_cargo_args
  end

  test do
    assert_match "pftui", shell_output("#{bin}/pftui --help")
  end
end
