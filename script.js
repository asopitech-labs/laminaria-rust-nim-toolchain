const observer = new IntersectionObserver((entries) => {
  entries.forEach((entry) => { if (entry.isIntersecting) entry.target.classList.add('visible'); });
}, { threshold: 0.12 });
document.querySelectorAll('.reveal').forEach((el) => observer.observe(el));

document.querySelectorAll('[data-copy]').forEach((button) => {
  button.addEventListener('click', async () => {
    try {
      await navigator.clipboard.writeText(button.dataset.copy);
      const original = button.textContent;
      button.textContent = document.documentElement.lang === 'ja' ? 'コピー済み' : 'copied';
      setTimeout(() => { button.textContent = original; }, 1200);
    } catch { button.textContent = document.documentElement.lang === 'ja' ? '選択' : 'select'; }
  });
});
