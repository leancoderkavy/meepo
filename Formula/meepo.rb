class Meepo < Formula
  desc "Local AI agent — connects Claude to your email, calendar, and more"
  homepage "https://github.com/kavymi/meepo"
  version "0.1.0"
  license "MIT"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/kavymi/meepo/releases/download/v#{version}/meepo-darwin-arm64.tar.gz"
      # sha256 will be filled after first release
      sha256 "PLACEHOLDER"
    else
      url "https://github.com/kavymi/meepo/releases/download/v#{version}/meepo-darwin-x64.tar.gz"
      sha256 "PLACEHOLDER"
    end
  end

  def install
    bin.install "meepo"
  end

  def caveats
    <<~EOS
      To get started, run the interactive setup:
        meepo setup

      Or initialize manually:
        meepo init
        export ANTHROPIC_API_KEY="sk-ant-..."
        meepo start
    EOS
  end

  test do
    assert_match "Meepo", shell_output("#{bin}/meepo --version")
  end
end
