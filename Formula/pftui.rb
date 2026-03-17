class Pftui < Formula
  desc "A beautiful terminal portfolio tracker with real-time prices, charts, and market data"
  homepage "https://github.com/skylarsimoncelli/pftui"
  url "https://github.com/skylarsimoncelli/pftui/archive/refs/tags/v0.12.0.tar.gz"
  sha256 "e4399f00f52860f1b853237d66a537509d0dd9535eecbb11c2fa18bf4d4a8d2b"
  license "MIT"

  depends_on "rust" => :build

  def install
    system "cargo", "install", *std_cargo_args
  end

  test do
    assert_match "pftui", shell_output("#{bin}/pftui --help")
  end
end
