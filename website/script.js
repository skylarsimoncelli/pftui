// ===== COPY TO CLIPBOARD =====
function copyInstall() {
    const text = document.getElementById('install-cmd').textContent;
    copyToClipboard(text);
    
    // Visual feedback: change icon to checkmark
    const button = event.target.closest('.copy-btn');
    if (!button) return;
    
    const originalSVG = button.innerHTML;
    button.innerHTML = '<svg width="16" height="16" viewBox="0 0 16 16" fill="none" xmlns="http://www.w3.org/2000/svg"><path d="M13.5 4L6 12L2.5 8.5" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/></svg>';
    button.style.color = 'var(--accent-green)';
    
    setTimeout(() => {
        button.innerHTML = originalSVG;
        button.style.color = '';
    }, 2000);
}

function copyCode(button) {
    const codeBlock = button.previousElementSibling;
    const text = codeBlock.textContent;
    copyToClipboard(text);
    
    const originalText = button.textContent;
    button.textContent = 'Copied!';
    button.style.background = 'var(--accent-green)';
    button.style.color = 'var(--bg-primary)';
    
    setTimeout(() => {
        button.textContent = originalText;
        button.style.background = '';
        button.style.color = '';
    }, 2000);
}

function copyToClipboard(text) {
    if (navigator.clipboard && window.isSecureContext) {
        navigator.clipboard.writeText(text);
    } else {
        const textArea = document.createElement('textarea');
        textArea.value = text;
        textArea.style.position = 'fixed';
        textArea.style.left = '-999999px';
        document.body.appendChild(textArea);
        textArea.select();
        try { document.execCommand('copy'); } catch (e) {}
        document.body.removeChild(textArea);
    }
}

// ===== INSTALLATION TABS =====
const installMethods = {
    curl: {
        title: 'curl install script',
        description: 'Fastest path for Linux/macOS. Re-run to upgrade.',
        command: 'curl -fsSL https://raw.githubusercontent.com/skylarsimoncelli/pftui/master/install.sh | bash'
    },
    brew: {
        title: 'Homebrew',
        description: 'Preferred on macOS.',
        command: 'brew tap skylarsimoncelli/pftui\nbrew install pftui'
    },
    cargo: {
        title: 'Cargo',
        description: 'Install directly from Rust crates.',
        command: 'cargo install pftui'
    },
    docker: {
        title: 'Docker',
        description: 'Run in an isolated container.',
        command: 'docker run -it ghcr.io/skylarsimoncelli/pftui:latest'
    },
    nix: {
        title: 'Nix',
        description: 'Run directly from the GitHub source with Nix.',
        command: 'nix run github:skylarsimoncelli/pftui'
    },
    apt: {
        title: 'apt (Debian/Ubuntu)',
        description: 'Install from the project package repository.',
        command: 'echo "deb [trusted=yes] https://skylarsimoncelli.github.io/pftui/apt stable main" | sudo tee /etc/apt/sources.list.d/pftui.list\nsudo apt update && sudo apt install pftui'
    },
    dnf: {
        title: 'dnf (Fedora/RHEL)',
        description: 'Install from the project RPM repository.',
        command: "sudo tee /etc/yum.repos.d/pftui.repo << 'EOF'\n[pftui]\nname=pftui\nbaseurl=https://skylarsimoncelli.github.io/pftui/rpm\nenabled=1\ngpgcheck=0\nEOF\nsudo dnf install pftui"
    }
};

function setInstallMethod(methodKey) {
    const method = installMethods[methodKey];
    if (!method) return;

    const title = document.getElementById('install-method-title');
    const description = document.getElementById('install-method-description');
    const code = document.getElementById('install-method-code');

    if (!title || !description || !code) return;

    title.textContent = method.title;
    description.textContent = method.description;
    code.textContent = method.command;

    document.querySelectorAll('.install-tab').forEach(tab => {
        const active = tab.dataset.install === methodKey;
        tab.classList.toggle('active', active);
        tab.setAttribute('aria-selected', active ? 'true' : 'false');
    });
}

function initInstallTabs() {
    const tabs = document.querySelectorAll('.install-tab');
    if (!tabs.length) return;

    tabs.forEach(tab => {
        tab.addEventListener('click', () => {
            setInstallMethod(tab.dataset.install);
        });
    });
}

function initHighlightsMarquee() {
    const scroller = document.getElementById('highlights-scroller');
    const marquee = document.getElementById('highlights-marquee');
    if (!scroller || !marquee || scroller.children.length > 1) return;

    const firstTrack = scroller.firstElementChild;
    if (!firstTrack) return;

    // Clone track for seamless loop
    const clone = firstTrack.cloneNode(true);
    clone.setAttribute('aria-hidden', 'true');
    scroller.appendChild(clone);

    // On mobile: pause auto-scroll during manual scroll, resume after
    const isMobile = window.matchMedia('(max-width: 768px)').matches;
    if (isMobile) {
        let scrollTimeout;
        marquee.addEventListener('scroll', () => {
            scroller.style.animationPlayState = 'paused';
            clearTimeout(scrollTimeout);
            scrollTimeout = setTimeout(() => {
                scroller.style.animationPlayState = 'running';
            }, 3000); // Resume auto-scroll 3s after last manual scroll
        }, { passive: true });

        // Touch events: pause during touch
        marquee.addEventListener('touchstart', () => {
            scroller.style.animationPlayState = 'paused';
        }, { passive: true });
        marquee.addEventListener('touchend', () => {
            clearTimeout(scrollTimeout);
            scrollTimeout = setTimeout(() => {
                scroller.style.animationPlayState = 'running';
            }, 3000);
        }, { passive: true });
    }
}

// ===== TERMINAL DEMO =====
// Fixed-height scenes: every scene has exactly 11 lines (command + blank + 8 content + closing)
// This prevents layout jitter from height changes between scenes.

const SCENE_LINES = 11;

const scenes = [
    {
        lines: [
            { text: '$ pftui', type: 'command', delay: 60 },
            { text: '', type: 'output', delay: 300 },
            { text: '┌─ Portfolio ──────────────────────────────┐', type: 'output', delay: 30 },
            { text: '│  Total Value    $287,345   ▲ +1.5%       │', type: 'output', delay: 30 },
            { text: '│  Day P&L        +$3,892                  │', type: 'output', delay: 30 },
            { text: '│                                          │', type: 'output', delay: 30 },
            { text: '│  Cash    35%  ██████████░░░░░░░░░░░░░   │', type: 'output', delay: 30 },
            { text: '│  Comd    30%  ███████░░░░░░░░░░░░░░░░   │', type: 'output', delay: 30 },
            { text: '│  Equity  10%  ████░░░░░░░░░░░░░░░░░░░   │', type: 'output', delay: 30 },
            { text: '└──────────────────────────────────────────┘', type: 'output', delay: 30 },
            { text: '', type: 'output', delay: 0 },
        ],
        hold: 3000,
    },
    {
        lines: [
            { text: '$ pftui macro', type: 'command', delay: 60 },
            { text: '', type: 'output', delay: 200 },
            { text: '┌─ Macro Intelligence ─────────────────────┐', type: 'output', delay: 30 },
            { text: '│  S&P 500    6,740  ▼ -1.3%     RSI 42   │', type: 'output', delay: 30 },
            { text: '│  VIX          29.5  ▲ +24.2%   ⚠️ HIGH   │', type: 'output', delay: 30 },
            { text: '│  DXY         98.86  ▼ -0.5%     RSI 56   │', type: 'output', delay: 30 },
            { text: '│  Gold       $5,181  ▲ +2.3%     RSI 58   │', type: 'output', delay: 30 },
            { text: '│  10Y Yield   4.13%  ▼ -2bps               │', type: 'output', delay: 30 },
            { text: '│  Oil WTI    $91.27  ▲ +12.7%    RSI 89 🔥│', type: 'output', delay: 30 },
            { text: '│  BTC       $68,290  ▼ -6.0%     RSI 38   │', type: 'output', delay: 30 },
            { text: '└──────────────────────────────────────────┘', type: 'output', delay: 30 },
        ],
        hold: 3000,
    },
    {
        lines: [
            { text: '$ pftui sentiment', type: 'command', delay: 60 },
            { text: '', type: 'output', delay: 200 },
            { text: '┌─ Market Sentiment ───────────────────────┐', type: 'output', delay: 30 },
            { text: '│  Crypto F&G     🔴 10  Extreme Fear      │', type: 'output', delay: 30 },
            { text: '│                                          │', type: 'output', delay: 30 },
            { text: '│  COT Positioning (Managed Money)         │', type: 'output', delay: 30 },
            { text: '│  Gold    🟢 Net Long   142k  (+8k)       │', type: 'output', delay: 30 },
            { text: '│  Silver  ⚠️ Net Long    38k  (-2k)       │', type: 'output', delay: 30 },
            { text: '│  Oil     🟢 Net Long   264k  (+19k)      │', type: 'output', delay: 30 },
            { text: '│  BTC     🔴 Net Short  -12k  (-8k)       │', type: 'output', delay: 30 },
            { text: '└──────────────────────────────────────────┘', type: 'output', delay: 30 },
        ],
        hold: 3000,
    },
    {
        lines: [
            { text: '$ pftui sector', type: 'command', delay: 60 },
            { text: '', type: 'output', delay: 200 },
            { text: '┌─ Sector ETF Performance ────────────────┐', type: 'output', delay: 30 },
            { text: '│  XLE  Energy         $89.42  ▲ +4.2%    │', type: 'output', delay: 30 },
            { text: '│  XLU  Utilities      $72.18  ▲ +1.8%    │', type: 'output', delay: 30 },
            { text: '│  XLP  Cons Staples   $78.95  ▲ +0.9%    │', type: 'output', delay: 30 },
            { text: '│  GDX  Gold Miners    $42.30  ▲ +3.1%    │', type: 'output', delay: 30 },
            { text: '│  XLF  Financials     $43.67  ▼ -1.2%    │', type: 'output', delay: 30 },
            { text: '│  XLK  Technology     $212.44 ▼ -2.1%    │', type: 'output', delay: 30 },
            { text: '│  SMH  Semiconductors $248.91 ▼ -3.7%    │', type: 'output', delay: 30 },
            { text: '└──────────────────────────────────────────┘', type: 'output', delay: 30 },
        ],
        hold: 3000,
    },
    {
        lines: [
            { text: '$ pftui calendar --impact high', type: 'command', delay: 60 },
            { text: '', type: 'output', delay: 200 },
            { text: '┌─ Economic Calendar (HIGH Impact) ───────┐', type: 'output', delay: 30 },
            { text: '│  🔴 Mar 12  CPI (Cons: 3.1%, Prev: 3.0%)│', type: 'output', delay: 30 },
            { text: '│  🔴 Mar 17  FOMC Rate Decision           │', type: 'output', delay: 30 },
            { text: '│  🔴 Mar 18  FOMC Press Conference        │', type: 'output', delay: 30 },
            { text: '│  🔴 Mar 27  GDP (Q4 Final)               │', type: 'output', delay: 30 },
            { text: '│  🔴 Apr 03  Non-Farm Payrolls            │', type: 'output', delay: 30 },
            { text: '│  🔴 Apr 10  CPI (April)                  │', type: 'output', delay: 30 },
            { text: '│                                          │', type: 'output', delay: 30 },
            { text: '└──────────────────────────────────────────┘', type: 'output', delay: 30 },
        ],
        hold: 3000,
    },
    {
        lines: [
            { text: '$ pftui supply', type: 'command', delay: 60 },
            { text: '', type: 'output', delay: 200 },
            { text: '┌─ COMEX Warehouse Inventory ─────────────┐', type: 'output', delay: 30 },
            { text: '│  Gold  (GC=F)                           │', type: 'output', delay: 30 },
            { text: '│  Registered:   16.2M troy oz             │', type: 'output', delay: 30 },
            { text: '│  Eligible:      2.0M troy oz             │', type: 'output', delay: 30 },
            { text: '│  Reg Ratio:     89.0%                    │', type: 'output', delay: 30 },
            { text: '│                                          │', type: 'output', delay: 30 },
            { text: '│  Silver  (SI=F)                          │', type: 'output', delay: 30 },
            { text: '│  Registered:   25.7M troy oz             │', type: 'output', delay: 30 },
            { text: '└──────────────────────────────────────────┘', type: 'output', delay: 30 },
        ],
        hold: 3000,
    },
    {
        lines: [
            { text: '$ pftui movers', type: 'command', delay: 60 },
            { text: '', type: 'output', delay: 200 },
            { text: '┌─ Significant Moves (>3%) ────────────────┐', type: 'output', delay: 30 },
            { text: '│  ▲ Oil WTI     +12.7%   Brent $90+      │', type: 'output', delay: 30 },
            { text: '│  ▲ VIX         +24.2%   War escalation   │', type: 'output', delay: 30 },
            { text: '│  ▲ Silver      +3.7%    Safe haven bid   │', type: 'output', delay: 30 },
            { text: '│  ▲ GDX         +3.1%    Gold miners      │', type: 'output', delay: 30 },
            { text: '│  ▼ BTC         -6.0%    Risk-off         │', type: 'output', delay: 30 },
            { text: '│  ▼ GLXY        -9.6%    Crypto proxy     │', type: 'output', delay: 30 },
            { text: '│  ▼ SMH         -3.7%    Semis selling    │', type: 'output', delay: 30 },
            { text: '└──────────────────────────────────────────┘', type: 'output', delay: 30 },
        ],
        hold: 3000,
    },
    {
        lines: [
            { text: '$ pftui eod', type: 'command', delay: 60 },
            { text: '', type: 'output', delay: 200 },
            { text: '┌─ End of Day Summary ────────────────────┐', type: 'output', delay: 30 },
            { text: '│  Portfolio   $287,345    ▲ +$3,892       │', type: 'output', delay: 30 },
            { text: '│  Movers      7 symbols > 3%              │', type: 'output', delay: 30 },
            { text: '│  VIX         29.5       ⚠️ Elevated       │', type: 'output', delay: 30 },
            { text: '│  F&G         🔴 10      Extreme Fear     │', type: 'output', delay: 30 },
            { text: '│  Oil         $91.27     RSI 89 🔥        │', type: 'output', delay: 30 },
            { text: '│  Gold COT    🟢 Managed Money Net Long   │', type: 'output', delay: 30 },
            { text: '│  Next Event  CPI Mar 12 (5 days)         │', type: 'output', delay: 30 },
            { text: '└──────────────────────────────────────────┘', type: 'output', delay: 30 },
        ],
        hold: 3000,
    },
    {
        lines: [
            { text: '$ pftui brief --agent --json', type: 'command', delay: 60 },
            { text: '', type: 'output', delay: 200 },
            { text: '{', type: 'json', delay: 20 },
            { text: '  "value": 287345.42,', type: 'json', delay: 20 },
            { text: '  "daily_pnl": 3892.18,', type: 'json', delay: 20 },
            { text: '  "movers": [{"sym":"OIL","chg":12.7},', type: 'json', delay: 20 },
            { text: '    {"sym":"VIX","chg":24.2},', type: 'json', delay: 20 },
            { text: '    {"sym":"BTC","chg":-6.0}],', type: 'json', delay: 20 },
            { text: '  "regime": "risk-off",', type: 'json', delay: 20 },
            { text: '  "fear_greed": 10,', type: 'json', delay: 20 },
            { text: '  "vix": 29.5, "dxy": 98.86', type: 'json', delay: 20 },
            { text: '}', type: 'json', delay: 20 },
        ],
        hold: 3000,
    },
];

let currentScene = 0;
let terminal = null;
let cursor = null;
let isAnimating = false;

function clearTerminal() {
    const lines = terminal.querySelectorAll('.terminal-line');
    lines.forEach(l => l.remove());
}

function createLine(text, type) {
    const line = document.createElement('div');
    line.className = 'terminal-line';
    
    if (type === 'command') {
        line.style.color = 'var(--accent-cyan)';
        line.style.fontWeight = '600';
    } else if (type === 'json') {
        line.style.color = 'var(--accent-yellow)';
        line.style.fontSize = '0.85em';
    }
    
    // Color code specific characters
    if (type === 'output' || type === 'json') {
        line.innerHTML = colorize(text);
    }
    
    return line;
}

function colorize(text) {
    return text
        .replace(/▲ \+[\d.]+%?/g, '<span style="color:var(--accent-green)">$&</span>')
        .replace(/▲\+\d+/g, '<span style="color:var(--accent-green)">$&</span>')
        .replace(/\+[\d.]+%/g, '<span style="color:var(--accent-green)">$&</span>')
        .replace(/▼ -[\d.]+%?/g, '<span style="color:var(--accent-red)">$&</span>')
        .replace(/▼-\d+/g, '<span style="color:var(--accent-red)">$&</span>')
        .replace(/-[\d.]+%/g, '<span style="color:var(--accent-red)">$&</span>')
        .replace(/→0/g, '<span style="color:var(--text-tertiary)">$&</span>')
        .replace(/(RSI \d+)/g, '<span style="color:var(--accent-blue)">$1</span>')
        .replace(/(█+)/g, '<span style="color:var(--accent-green)">$1</span>')
        .replace(/(░+)/g, '<span style="color:var(--bg-tertiary)">$1</span>')
        .replace(/(\d+%)\s/g, '<span style="color:var(--accent-cyan)">$1</span> ')
        .replace(/([\d,]+\.\d+|\$[\d,]+)/g, '<span style="color:var(--text-primary);font-weight:500">$1</span>')
        .replace(/(⚠️ HIGH|🔥)/g, '<span style="color:var(--accent-red)">$1</span>')
        .replace(/(Extreme Fear)/g, '<span style="color:var(--accent-red)">$1</span>')
        .replace(/(🟢|🔴|⚠️)/g, '$1');
}

async function typeText(element, text, delay) {
    for (let i = 0; i <= text.length; i++) {
        element.textContent = text.slice(0, i);
        await sleep(delay);
    }
}

async function playScene(scene) {
    clearTerminal();
    cursor.style.display = 'inline-block';
    
    for (const line of scene.lines) {
        const el = createLine('', line.type);
        terminal.insertBefore(el, cursor);
        
        if (line.type === 'command') {
            // Type out commands character by character
            await typeText(el, line.text, line.delay);
            await sleep(400);
            cursor.style.display = 'none';
            await sleep(200);
        } else {
            // Output lines appear instantly (like real terminal output)
            if (line.type === 'json') {
                el.innerHTML = line.text;
                el.style.color = 'var(--accent-yellow)';
            } else {
                el.innerHTML = colorize(line.text);
            }
            await sleep(line.delay);
        }
    }
    
    // Show cursor blinking after scene completes
    cursor.style.display = 'inline-block';
    
    // Hold the scene
    await sleep(scene.hold);
}

async function runTerminalLoop() {
    if (isAnimating) return;
    isAnimating = true;
    
    while (true) {
        await playScene(scenes[currentScene]);
        
        // Fade transition
        terminal.style.opacity = '0.3';
        await sleep(300);
        terminal.style.opacity = '1';
        
        currentScene = (currentScene + 1) % scenes.length;
    }
}

function sleep(ms) {
    return new Promise(resolve => setTimeout(resolve, ms));
}

// ===== INTERSECTION OBSERVER FOR ANIMATIONS =====
const observerOptions = {
    threshold: 0.1,
    rootMargin: '0px 0px -50px 0px'
};

const observer = new IntersectionObserver((entries) => {
    entries.forEach(entry => {
        if (entry.isIntersecting) {
            entry.target.classList.add('visible');
        }
    });
}, observerOptions);

// ===== SMOOTH SCROLL =====
document.querySelectorAll('a[href^="#"]').forEach(anchor => {
    anchor.addEventListener('click', function (e) {
        e.preventDefault();
        const target = document.querySelector(this.getAttribute('href'));
        if (target) {
            target.scrollIntoView({ behavior: 'smooth', block: 'start' });
        }
    });
});

// ===== INIT =====
window.addEventListener('DOMContentLoaded', () => {
    terminal = document.getElementById('terminal');
    cursor = terminal.querySelector('.terminal-cursor');
    
    // Add transition for fade effect
    terminal.style.transition = 'opacity 0.3s ease';
    
    // Start terminal demo after a brief pause
    setTimeout(runTerminalLoop, 800);

    initHighlightsMarquee();
    initInstallTabs();
    setInstallMethod('curl');
    
    // Observe fade-in elements
    document.querySelectorAll('.fade-in, .highlight-card').forEach(el => {
        observer.observe(el);
    });
});
