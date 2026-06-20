# Homebrew formula for tarn — builds from source (zero crate deps, so this is
# fast and always correct; no per-release binary sha juggling).
#
# To ship it: create a tap repo `OneMuppet/homebrew-tap`, drop this file in as
# `Formula/tarn.rb`, set the `url`/`sha256` for the tagged release, and users get
# `brew install onemuppet/tap/tarn`. (Or point `url` at a release tarball.)
#
# The crate is published as `tarn-cli` but the installed command is `tarn`.
class Tarn < Formula
  desc "Tiny terminal editor and structural CLI toolkit built for AI agents"
  homepage "https://github.com/OneMuppet/tarn"
  # Update per release: the source tarball for the tag, and its sha256.
  url "https://github.com/OneMuppet/tarn/archive/refs/tags/v0.1.0.tar.gz"
  sha256 "66eb77f10269e3dc6750d7abc9d255f2ddccf6032566d0f3d1ece29011c2649c"
  license "MIT"
  head "https://github.com/OneMuppet/tarn.git", branch: "main"

  depends_on "rust" => :build

  def install
    # [[bin]] name = "tarn", so this installs the `tarn` command.
    system "cargo", "install", *std_cargo_args
  end

  test do
    assert_match "tarn", shell_output("#{bin}/tarn --version")
    # exercise the no-TTY scriptable path end to end
    (testpath/"x.txt").write("alpha\nbeta\n")
    assert_equal "1", shell_output("#{bin}/tarn find #{testpath}/x.txt beta -c").strip
  end
end
