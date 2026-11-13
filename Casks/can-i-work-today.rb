cask "can-i-work-today" do
  version "0.1.0"
  sha256 "c68575a3188623d99ebd726c628709ad2b9bf69ccc3f4af38cc0f5e86ec5d1f3"

  url "https://github.com/Mackaber/can-i-work-today/releases/download/v#{version}/Can.I.Work.Today_#{version}_aarch64.dmg"
  name "Can I Work Today"
  desc "System tray app that monitors the health of cloud and SaaS services"
  homepage "https://github.com/Mackaber/can-i-work-today"

  depends_on arch: :arm64
  depends_on :macos

  app "Can I Work Today.app"
end
