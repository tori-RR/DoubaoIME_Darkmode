(function () {
  function greetingWord(now) {
    const d = now || new Date();
    const m = d.getHours() * 60 + d.getMinutes();
    if (m >= 6 * 60 && m < 10 * 60) return "早上好";
    if (m >= 10 * 60 && m < 11 * 60 + 30) return "上午好";
    if (m >= 11 * 60 + 30 && m < 14 * 60) return "中午好";
    if (m >= 14 * 60 && m < 18 * 60 + 30) return "下午好";
    return "晚上好";
  }

  function paintGreeting() {
    const el = document.getElementById("greet-word");
    if (el) el.textContent = greetingWord();
  }

  paintGreeting();
  setInterval(paintGreeting, 30 * 1000);
  document.addEventListener("visibilitychange", () => {
    if (!document.hidden) paintGreeting();
  });
})();
