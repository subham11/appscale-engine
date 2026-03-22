class Appscale < Formula
  desc "Cross-platform React UI engine — build native apps with one codebase"
  homepage "https://github.com/subham11/appscale-engine"
  version "0.1.0"
  license "MIT"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/subham11/appscale-engine/releases/download/v#{version}/appscale-v#{version}-aarch64-apple-darwin.tar.gz"
      sha256 "PLACEHOLDER_ARM64_SHA256"
    else
      url "https://github.com/subham11/appscale-engine/releases/download/v#{version}/appscale-v#{version}-x86_64-apple-darwin.tar.gz"
      sha256 "PLACEHOLDER_X86_64_SHA256"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://github.com/subham11/appscale-engine/releases/download/v#{version}/appscale-v#{version}-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "PLACEHOLDER_LINUX_ARM64_SHA256"
    else
      url "https://github.com/subham11/appscale-engine/releases/download/v#{version}/appscale-v#{version}-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "PLACEHOLDER_LINUX_X86_64_SHA256"
    end
  end

  def install
    bin.install "appscale"
  end

  test do
    assert_match "appscale", shell_output("#{bin}/appscale --version")
  end
end
