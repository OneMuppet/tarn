# Homebrew formula for tarn.
#
# Installs a prebuilt binary on the platforms we ship (Apple Silicon macOS and
# x86_64 Linux) — no Rust toolchain needed. Intel macOS has no prebuilt binary,
# so it falls back to building from source (needs Rust, pulled in as a build dep).
#
# To ship updates: bump `version`, update the three URLs + sha256 values, drop this
# file into the tap repo `OneMuppet/homebrew-tap` as `Formula/tarn.rb`. Users get
# `brew install onemuppet/tap/tarn`.
class Tarn < Formula
  desc "Tiny terminal editor and structural CLI toolkit built for AI agents"
  homepage "https://github.com/OneMuppet/tarn"
  version "0.9.3"
  license "MIT"
  head "https://github.com/OneMuppet/tarn.git", branch: "main"

  on_macos do
    on_arm do
      url "https://github.com/OneMuppet/tarn/releases/download/v0.9.3/tarn-v0.9.3-aarch64-apple-darwin.tar.gz"
      sha256 "9c02d2965f4d981385aa18a0fecf6ec8402c4316e37fc8ea98d8c2c930ddd301"
    end
    on_intel do
      # No prebuilt Intel-mac binary — build from the tagged source.
      url "https://github.com/OneMuppet/tarn/archive/refs/tags/v0.9.3.tar.gz"
      sha256 "6f418eb4557163a1e28649b35e2febf304e06d43dc6d8e68a660eff411e07ffb"
      depends_on "rust" => :build
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/OneMuppet/tarn/releases/download/v0.9.3/tarn-v0.9.3-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "463af479a1e65f388989c72acc3331949cc3d565f1237218159be98b5c103241"
    end
    on_arm do
      # arm64 Linux binaries ship from v0.9.3 (cloud sandboxes / ARM CI / Graviton).
      url "https://github.com/OneMuppet/tarn/releases/download/v0.9.3/tarn-v0.9.3-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "54bae94c96b789507289aa22df9e81e1237a161536f61364e3891394009ba451"
    end
  end

  def install
    # Source builds (Intel mac, `--HEAD`) have a Cargo.toml; binary tarballs are
    # just the `tarn` executable. [[bin]] name = "tarn" keeps the command `tarn`.
    if File.exist?("Cargo.toml")
      system "cargo", "install", *std_cargo_args
    else
      bin.install "tarn"
    end
  end

  test do
    assert_match "tarn", shell_output("#{bin}/tarn --version")
    # exercise the no-TTY scriptable path end to end
    (testpath/"x.txt").write("alpha\nbeta\n")
    assert_equal "1", shell_output("#{bin}/tarn find #{testpath}/x.txt beta -c").strip
  end
end
