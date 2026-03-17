class Pftui < Formula
  desc "A beautiful terminal portfolio tracker with real-time prices, charts, and market data"
  homepage "https://github.com/skylarsimoncelli/pftui"
  url "https://github.com/skylarsimoncelli/pftui/archive/refs/tags/v0.12.1.tar.gz"
  sha256 "2d489e63ec311410e676c5e286d1886cce8f13db17cca7c372fc71743d999ba4"
  license "MIT"

  depends_on "rust" => :build

  def install
    system "cargo", "install", *std_cargo_args
  end

  test do
    assert_match "pftui", shell_output("#{bin}/pftui --help")
  end
end
