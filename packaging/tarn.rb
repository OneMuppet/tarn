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
  version "0.8.1"
  license "MIT"
  head "https://github.com/OneMuppet/tarn.git", branch: "main"

  on_macos do
    on_arm do
      url "https://github.com/OneMuppet/tarn/releases/download/v0.8.1/tarn-v0.8.1-aarch64-apple-darwin.tar.gz"
      sha256 "273df382e62a0e5fde4feb9c4d946eab23b28aa474b99e143b8990bf678e4cf3"
    end
    on_intel do
      # No prebuilt Intel-mac binary — build from the tagged source.
      url "https://github.com/OneMuppet/tarn/archive/refs/tags/v0.8.1.tar.gz"
      sha256 "1ac22b6e875110b04a936e5a490b081166e6f880c253f6466c9e259edb686e07"
      depends_on "rust" => :build
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/OneMuppet/tarn/releases/download/v0.8.1/tarn-v0.8.1-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "afa96abb3fc8dae5ee5b70124836a4cb9ea15561ca55c74ffe276cc8eae2bd91"
    end
    on_arm do
      # arm64 Linux binaries ship from v0.9.0 (cloud sandboxes / ARM CI / Graviton).
      url "https://github.com/OneMuppet/tarn/releases/download/v0.9.0/tarn-v0.9.0-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "FILLED_AT_V090_RELEASE"
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
