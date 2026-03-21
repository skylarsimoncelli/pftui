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
            { text: '$ pftui analytics situation', type: 'command', delay: 60 },
            { text: '┌─ Situation Room ────────────────────────┐', type: 'output', delay: 30 },
            { text: '│  REGIME     Risk-off (75% confidence)   │', type: 'output', delay: 30 },
            { text: '│  HEADLINE   bearish alignment (3/4)     │', type: 'output', delay: 30 },
            { text: '│  WATCH NOW                              │', type: 'output', delay: 30 },
            { text: '│  ⚠ 3 live alerts              critical  │', type: 'output', delay: 30 },
            { text: '│  ⚠ ^VIX leading the tape      elevated  │', type: 'output', delay: 30 },
            { text: '└──────────────────────────────────────────┘', type: 'output', delay: 30 },
        ],
        hold: 3000,
    },
    {
        lines: [
            { text: '$ pftui analytics impact', type: 'command', delay: 60 },
            { text: '┌─ Portfolio Impact ───────────────────────┐', type: 'output', delay: 30 },
            { text: '│  SYM   CONSENSUS  SCORE  EVIDENCE       │', type: 'output', delay: 30 },
            { text: '│  GLD   bullish    142    2 bull layers   │', type: 'output', delay: 30 },
            { text: '│                          Conviction +4   │', type: 'output', delay: 30 },
            { text: '│  BTC   bullish    154    2 bull layers   │', type: 'output', delay: 30 },
            { text: '│                          SMA 200 reclaim │', type: 'output', delay: 30 },
            { text: '└──────────────────────────────────────────┘', type: 'output', delay: 30 },
        ],
        hold: 3000,
    },
    {
        lines: [
            { text: '$ pftui analytics synthesis', type: 'command', delay: 60 },
            { text: '┌─ Cross-Timeframe Synthesis ─────────────┐', type: 'output', delay: 30 },
            { text: '│  ALIGNMENT (all layers agree)           │', type: 'output', delay: 30 },
            { text: '│  GOOG  LOW:bear MED:bear HIGH:bear      │', type: 'output', delay: 30 },
            { text: '│        → BEARISH consensus (3/4)        │', type: 'output', delay: 30 },
            { text: '│  DIVERGENCE (investigate)               │', type: 'output', delay: 30 },
            { text: '│  BTC   LOW:bear MED:bull HIGH:bull      │', type: 'output', delay: 30 },
            { text: '└──────────────────────────────────────────┘', type: 'output', delay: 30 },
        ],
        hold: 3000,
    },
    {
        lines: [
            { text: '$ pftui data refresh', type: 'command', delay: 60 },
            { text: '  ✓ Prices         84 symbols, technicals │', type: 'output', delay: 30 },
            { text: '  ✓ Correlations   33 cross-asset pairs   │', type: 'output', delay: 30 },
            { text: '  ✓ FedWatch       14 FOMC meetings       │', type: 'output', delay: 30 },
            { text: '  ✓ COT            4 CFTC reports         │', type: 'output', delay: 30 },
            { text: '  ✓ Economy        101 BLS series         │', type: 'output', delay: 30 },
            { text: '  ✓ Sovereign      CB gold, govt BTC      │', type: 'output', delay: 30 },
            { text: '  19 sources refreshed, 371 signals       │', type: 'output', delay: 30 },
        ],
        hold: 3000,
    },
    {
        lines: [
            { text: '$ pftui analytics catalysts', type: 'command', delay: 60 },
            { text: '┌─ Ranked Catalysts ──────────────────────┐', type: 'output', delay: 30 },
            { text: '│  THIS WEEK                              │', type: 'output', delay: 30 },
            { text: '│  GDP (Preliminary)    HIGH   score:18   │', type: 'output', delay: 30 },
            { text: '│    → SPY, QQQ, CL=F affected            │', type: 'output', delay: 30 },
            { text: '│  Durable Goods        MED    score:8    │', type: 'output', delay: 30 },
            { text: '│  PCE Price Index      HIGH   score:22   │', type: 'output', delay: 30 },
            { text: '└──────────────────────────────────────────┘', type: 'output', delay: 30 },
        ],
        hold: 3000,
    },
    {
        lines: [
            { text: '$ pftui analytics opportunities', type: 'command', delay: 60 },
            { text: '┌─ Non-Held Opportunities ────────────────┐', type: 'output', delay: 30 },
            { text: '│  CL=F  Crude Oil   bullish  score:91    │', type: 'output', delay: 30 },
            { text: '│         2 bull layers, conviction +4    │', type: 'output', delay: 30 },
            { text: '│         Catalyst: GDP this week         │', type: 'output', delay: 30 },
            { text: '│  CCJ   Cameco      bullish  score:80    │', type: 'output', delay: 30 },
            { text: '│         Nuclear Renaissance trend       │', type: 'output', delay: 30 },
            { text: '└──────────────────────────────────────────┘', type: 'output', delay: 30 },
        ],
        hold: 3000,
    },
    {
        lines: [
            { text: '$ pftui analytics movers', type: 'command', delay: 60 },
            { text: '┌─ Significant Moves (>3%) ───────────────┐', type: 'output', delay: 30 },
            { text: '│  ▲ Oil WTI     +12.7%  Brent $90+       │', type: 'output', delay: 30 },
            { text: '│  ▲ VIX         +24.2%  Vol spike         │', type: 'output', delay: 30 },
            { text: '│  ▲ Silver      +3.7%   Safe haven bid    │', type: 'output', delay: 30 },
            { text: '│  ▼ BTC         -6.0%   Risk-off          │', type: 'output', delay: 30 },
            { text: '│  ▼ GLXY        -9.6%   Crypto proxy      │', type: 'output', delay: 30 },
            { text: '└──────────────────────────────────────────┘', type: 'output', delay: 30 },
        ],
        hold: 3000,
    },
    {
        lines: [
            { text: '$ pftui data calendar --impact high', type: 'command', delay: 60 },
            { text: '┌─ Economic Calendar (HIGH Impact) ───────┐', type: 'output', delay: 30 },
            { text: '│  🔴 Mar 12  CPI (Cons 3.1%, Prev 3.0%)  │', type: 'output', delay: 30 },
            { text: '│  🔴 Mar 17  FOMC Rate Decision           │', type: 'output', delay: 30 },
            { text: '│  🔴 Mar 27  GDP (Q4 Final)               │', type: 'output', delay: 30 },
            { text: '│  🔴 Apr 03  Non-Farm Payrolls            │', type: 'output', delay: 30 },
            { text: '│  🔴 Apr 10  CPI (April)                  │', type: 'output', delay: 30 },
            { text: '└──────────────────────────────────────────┘', type: 'output', delay: 30 },
        ],
        hold: 3000,
    },
    {
        lines: [
            { text: '$ pftui journal prediction stats', type: 'command', delay: 60 },
            { text: '{', type: 'json', delay: 20 },
            { text: '  "total": 165, "scored": 102,', type: 'json', delay: 20 },
            { text: '  "correct": 46, "hit_rate_pct": 45.1,', type: 'json', delay: 20 },
            { text: '  "by_conviction": {', type: 'json', delay: 20 },
            { text: '    "high":  {"scored":29,"hit_rate":48.3}', type: 'json', delay: 20 },
            { text: '    "medium":{"scored":64,"hit_rate":43.8}', type: 'json', delay: 20 },
            { text: '  }', type: 'json', delay: 20 },
        ],
        hold: 3000,
    },
    {
        lines: [
            { text: '$ pftui journal scenario update "Stagflation" --probability 38', type: 'command', delay: 30 },
            { text: '✓ updated scenario: Stagflation (38%)', type: 'output', delay: 30 },
            { text: '$ pftui journal conviction set GLD --score 4', type: 'command', delay: 30 },
            { text: '✓ conviction logged: GLD = +4', type: 'output', delay: 30 },
            { text: '$ pftui agent message send "all 4 layers bullish"', type: 'command', delay: 30 },
            { text: '✓ message queued for operator review', type: 'output', delay: 30 },
            { text: '$ pftui analytics alignment --symbol GLD', type: 'command', delay: 30 },
            { text: 'GLD  bull bull bull neutral  BULLISH 75%', type: 'output', delay: 30 },
        ],
        hold: 3200,
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

// ===== SCREENSHOT GALLERY =====
function initGallery() {
    const tabs = document.querySelectorAll('.gallery-tab');
    const slides = document.querySelectorAll('.gallery-slide');
    if (!tabs.length) return;

    tabs.forEach(tab => {
        tab.addEventListener('click', () => {
            const target = tab.dataset.target;
            tabs.forEach(t => t.classList.remove('active'));
            slides.forEach(s => s.classList.remove('active'));
            tab.classList.add('active');
            document.getElementById(target).classList.add('active');
        });
    });
}

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
    initGallery();
    
    // Observe fade-in elements
    document.querySelectorAll('.fade-in, .highlight-card').forEach(el => {
        observer.observe(el);
    });
});
