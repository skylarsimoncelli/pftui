class Pftui < Formula
  desc "A beautiful terminal portfolio tracker with real-time prices, charts, and market data"
  homepage "https://github.com/skylarsimoncelli/pftui"
  license "MIT"
  version "0.1.0"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/skylarsimoncelli/pftui/releases/download/v0.1.0/pftui-aarch64-macos"
      sha256 "8352c9a489827ea87c793b10b2a9fb8a7825ffc95b2a110676de0097f43d7980"
    else
      url "https://github.com/skylarsimoncelli/pftui/releases/download/v0.1.0/pftui-x86_64-macos"
      sha256 "f495f06426e1be5dc09703009527ba14815462bd4a9a05820c431e9316882f80"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://github.com/skylarsimoncelli/pftui/releases/download/v0.1.0/pftui-aarch64-linux"
      sha256 "8aa34c97450f5a54f7932bd025b2d7c82287e8fd7953ac3344b8109d17dc3303"
    else
      url "https://github.com/skylarsimoncelli/pftui/releases/download/v0.1.0/pftui-x86_64-linux"
      sha256 "60866ee90f417b8daa14d7e45f8790c2f072e16ca0c5f3619387e61f7645476e"
    end
  end

  def install
    binary = Dir["pftui*"].first
    mv binary, "pftui"
    bin.install "pftui"
  end

  test do
    assert_match "pftui", shell_output("#{bin}/pftui --help")
  end
end
