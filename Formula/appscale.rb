class Appscale < Formula
  desc "Cross-platform React UI engine — build native apps with one codebase"
  homepage "https://github.com/subham11/appscale-engine"
  version "0.1.0"
  license "MIT"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/subham11/appscale-engine/releases/download/v#{version}/appscale-v#{version}-aarch64-apple-darwin.tar.gz"
      sha256 "664684e5474c2cd51002d3ea989efad7a15c339ca7713c51aeddf9ee8a2d8d49"
    else
      url "https://github.com/subham11/appscale-engine/releases/download/v#{version}/appscale-v#{version}-x86_64-apple-darwin.tar.gz"
      sha256 "51655976304dfd27d31331ec3eaa26d8f511677400281f574fc3581cbcad8b80"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://github.com/subham11/appscale-engine/releases/download/v#{version}/appscale-v#{version}-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "5f0aff631333142883367428b9f1a5d527085aa388c67a4f48969c8c6d0c4329"
    else
      url "https://github.com/subham11/appscale-engine/releases/download/v#{version}/appscale-v#{version}-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "dbcb173e895675852bc217687f158bd286b6af62183226144e7c73217d2c79bb"
    end
  end

  def install
    bin.install "appscale"
  end

  test do
    assert_match "appscale", shell_output("#{bin}/appscale --version")
  end
end
