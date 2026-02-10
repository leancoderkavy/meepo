class Meepo < Formula
  desc "Local AI agent — connects Claude to your email, calendar, and more"
  homepage "https://github.com/kavymi/meepo"
  version "0.1.1"
  license "MIT"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/kavymi/meepo/releases/download/v#{version}/meepo-darwin-arm64.tar.gz"
      sha256 "e60b7d93064c3e1fdcb4671afa45c331f7af29bc5bd4c8754d9f11bf08e590bc"
    else
      url "https://github.com/kavymi/meepo/releases/download/v#{version}/meepo-darwin-x64.tar.gz"
      sha256 "3d0e21c52b485e127b97a43cfa5aa03f1791cdadf4a29baa8caa140aa6184adc"
    end
  end

  def install
    bin.install "meepo"
  end

  def caveats
    <<~EOS
      Run the setup wizard to configure API keys:
        meepo setup

      Then start the agent:
        meepo start

      Enable channels (Discord, Slack, iMessage) in:
        ~/.meepo/config.toml
    EOS
  end

  test do
    assert_match "Meepo", shell_output("#{bin}/meepo --version")
  end
end
