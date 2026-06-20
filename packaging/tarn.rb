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
  version "0.2.0"
  license "MIT"
  head "https://github.com/OneMuppet/tarn.git", branch: "main"

  on_macos do
    on_arm do
      url "https://github.com/OneMuppet/tarn/releases/download/v0.2.0/tarn-v0.2.0-aarch64-apple-darwin.tar.gz"
      sha256 "71b57ceac1c7bc0651f1a1e411c41d5b4178993c3729334597bfc5ca5693218c"
    end
    on_intel do
      # No prebuilt Intel-mac binary — build from the tagged source.
      url "https://github.com/OneMuppet/tarn/archive/refs/tags/v0.2.0.tar.gz"
      sha256 "1042fa5db87295716474131ef72ffd72d813a80f51b5c9504c93cc4021d3239c"
      depends_on "rust" => :build
    end
  end

  on_linux do
    url "https://github.com/OneMuppet/tarn/releases/download/v0.2.0/tarn-v0.2.0-x86_64-unknown-linux-gnu.tar.gz"
    sha256 "c95c3bc7fe08de861d55f23d2bf41fa8deadc6eccde15151f26dffe095830430"
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
