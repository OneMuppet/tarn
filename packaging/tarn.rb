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
  version "0.7.1"
  license "MIT"
  head "https://github.com/OneMuppet/tarn.git", branch: "main"

  on_macos do
    on_arm do
      url "https://github.com/OneMuppet/tarn/releases/download/v0.7.1/tarn-v0.7.1-aarch64-apple-darwin.tar.gz"
      sha256 "77fff866ecad2fa23686f72ac3250cc077531ebf76e4b400cee8a9aeb6925a04"
    end
    on_intel do
      # No prebuilt Intel-mac binary — build from the tagged source.
      url "https://github.com/OneMuppet/tarn/archive/refs/tags/v0.7.1.tar.gz"
      sha256 "62b44bca3743d93acb2d0dd2098046ff370254b035febaa0bca8d11843a4706d"
      depends_on "rust" => :build
    end
  end

  on_linux do
    url "https://github.com/OneMuppet/tarn/releases/download/v0.7.1/tarn-v0.7.1-x86_64-unknown-linux-gnu.tar.gz"
    sha256 "2e715917909529bcaa722f95949dec8138a5a3a2a8183e1683990a46badc1fa0"
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
