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
  version "0.5.0"
  license "MIT"
  head "https://github.com/OneMuppet/tarn.git", branch: "main"

  on_macos do
    on_arm do
      url "https://github.com/OneMuppet/tarn/releases/download/v0.5.0/tarn-v0.5.0-aarch64-apple-darwin.tar.gz"
      sha256 "4bd35de32d3bc69ad320b88fce3240ff1a679a3c468b180dfdfc10705a5adc3a"
    end
    on_intel do
      # No prebuilt Intel-mac binary — build from the tagged source.
      url "https://github.com/OneMuppet/tarn/archive/refs/tags/v0.5.0.tar.gz"
      sha256 "990bc6e6f93dbba2cf4f1d924563a96ccada5715b0d605562a03c3eb803e4cc0"
      depends_on "rust" => :build
    end
  end

  on_linux do
    url "https://github.com/OneMuppet/tarn/releases/download/v0.5.0/tarn-v0.5.0-x86_64-unknown-linux-gnu.tar.gz"
    sha256 "15071d857de8b5acef4e5ed2b9ab638ddc7116a6acccb6479ed9618be417902a"
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
