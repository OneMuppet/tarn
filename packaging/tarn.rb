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
  version "0.9.1"
  license "MIT"
  head "https://github.com/OneMuppet/tarn.git", branch: "main"

  on_macos do
    on_arm do
      url "https://github.com/OneMuppet/tarn/releases/download/v0.9.1/tarn-v0.9.1-aarch64-apple-darwin.tar.gz"
      sha256 "c8f3f9e186b912e27f3c434a5a64eca4acaacc7ffad3ef789248a6ebb1952347"
    end
    on_intel do
      # No prebuilt Intel-mac binary — build from the tagged source.
      url "https://github.com/OneMuppet/tarn/archive/refs/tags/v0.9.1.tar.gz"
      sha256 "bb45b13a6feffe46707e0c6077cc23ab9f466356eb02987aa5cb379d9b9c0480"
      depends_on "rust" => :build
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/OneMuppet/tarn/releases/download/v0.9.1/tarn-v0.9.1-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "723b64e30d9fae736b9c1179f9d657f9b5d5dcfdbfddda6134816192e2556baf"
    end
    on_arm do
      # arm64 Linux binaries ship from v0.9.1 (cloud sandboxes / ARM CI / Graviton).
      url "https://github.com/OneMuppet/tarn/releases/download/v0.9.1/tarn-v0.9.1-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "60b059f4775bd6beb0b73831000b2dd9a807c1927ec1d0066ef4d048db8903da"
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
