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
  version "0.9.2"
  license "MIT"
  head "https://github.com/OneMuppet/tarn.git", branch: "main"

  on_macos do
    on_arm do
      url "https://github.com/OneMuppet/tarn/releases/download/v0.9.2/tarn-v0.9.2-aarch64-apple-darwin.tar.gz"
      sha256 "e83e1797b7754bbd43371b628222bad3ee4d6da31134bb0335118a6e9645496b"
    end
    on_intel do
      # No prebuilt Intel-mac binary — build from the tagged source.
      url "https://github.com/OneMuppet/tarn/archive/refs/tags/v0.9.2.tar.gz"
      sha256 "c884ca97854075d8c78ea8ec14bc21cd1b825cb97fc935f8ab85bc35125ba717"
      depends_on "rust" => :build
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/OneMuppet/tarn/releases/download/v0.9.2/tarn-v0.9.2-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "ead016503dcc2cf519f2aebf49045e164aae443721c2f985bcb193f8d3eaec48"
    end
    on_arm do
      # arm64 Linux binaries ship from v0.9.2 (cloud sandboxes / ARM CI / Graviton).
      url "https://github.com/OneMuppet/tarn/releases/download/v0.9.2/tarn-v0.9.2-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "0e6dc5a907568b9149d2383fa1c8c62373af1c9c52f874e376ecd4a76656ec21"
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
