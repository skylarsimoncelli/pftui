// ===== COPY TO CLIPBOARD =====
function copyInstall() {
    const text = document.getElementById('install-cmd').textContent;
    copyToClipboard(text);
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
        description: 'Fastest path for Linux/macOS.',
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
    if (!scroller || scroller.children.length > 1) return;

    const firstTrack = scroller.firstElementChild;
    if (!firstTrack) return;

    const clone = firstTrack.cloneNode(true);
    clone.setAttribute('aria-hidden', 'true');
    scroller.appendChild(clone);
}

// ===== TERMINAL DEMO =====
// Multi-scene terminal that cycles through pftui features

const scenes = [
    {
        lines: [
            { text: '$ pftui', type: 'command', delay: 60 },
            { text: '', type: 'output', delay: 300 },
            { text: '┌─ Portfolio ──────────────────────────────┐', type: 'output', delay: 30 },
            { text: '│  Total Value    $48,217    ▲ +2.4%       │', type: 'output', delay: 30 },
            { text: '│  Day P&L        +$1,132                  │', type: 'output', delay: 30 },
            { text: '│                                          │', type: 'output', delay: 30 },
            { text: '│  Equity   45%  ██████████░░░░░░░░░░░░   │', type: 'output', delay: 30 },
            { text: '│  Crypto   30%  ██████░░░░░░░░░░░░░░░░   │', type: 'output', delay: 30 },
            { text: '│  Comd     25%  █████░░░░░░░░░░░░░░░░░   │', type: 'output', delay: 30 },
            { text: '└──────────────────────────────────────────┘', type: 'output', delay: 30 },
        ],
        hold: 3000,
    },
    {
        lines: [
            { text: '$ pftui macro', type: 'command', delay: 60 },
            { text: '', type: 'output', delay: 200 },
            { text: '┌─ Market Intelligence ────────────────────┐', type: 'output', delay: 30 },
            { text: '│  S&P 500    ▲ +0.8%            RSI 58    │', type: 'output', delay: 30 },
            { text: '│  VIX        ▼ -10.3%                      │', type: 'output', delay: 30 },
            { text: '│  DXY        ▼ -0.1%            RSI 42    │', type: 'output', delay: 30 },
            { text: '│  Gold       ▲ +1.2%            RSI 67    │', type: 'output', delay: 30 },
            { text: '│  10Y Yield  ▼ -3bps                       │', type: 'output', delay: 30 },
            { text: '│  Oil WTI    ▲ +0.6%                       │', type: 'output', delay: 30 },
            { text: '│  BTC        ▲ +3.1%            RSI 61    │', type: 'output', delay: 30 },
            { text: '└──────────────────────────────────────────┘', type: 'output', delay: 30 },
        ],
        hold: 3000,
    },
    {
        lines: [
            { text: '$ pftui brief --agent', type: 'command', delay: 60 },
            { text: '', type: 'output', delay: 200 },
            { text: '{', type: 'json', delay: 20 },
            { text: '  "portfolio_value": 48217.43,', type: 'json', delay: 20 },
            { text: '  "daily_pnl": 1132.18,', type: 'json', delay: 20 },
            { text: '  "daily_pnl_pct": 2.4,', type: 'json', delay: 20 },
            { text: '  "top_movers": [', type: 'json', delay: 20 },
            { text: '    {"symbol": "BTC", "change": 3.1},', type: 'json', delay: 20 },
            { text: '    {"symbol": "GOLD", "change": 1.2}', type: 'json', delay: 20 },
            { text: '  ],', type: 'json', delay: 20 },
            { text: '  "alerts_triggered": 0', type: 'json', delay: 20 },
            { text: '}', type: 'json', delay: 20 },
        ],
        hold: 3000,
    },
    {
        lines: [
            { text: '$ pftui predictions', type: 'command', delay: 60 },
            { text: '', type: 'output', delay: 200 },
            { text: '┌─ Prediction Markets ─────────────────────┐', type: 'output', delay: 30 },
            { text: '│  Fed rate cut next meeting?   67%   ▲+4  │', type: 'output', delay: 30 },
            { text: '│  BTC breaks resistance soon?  52%   ▼-3  │', type: 'output', delay: 30 },
            { text: '│  Recession odds rise?         23%   ▲+2  │', type: 'output', delay: 30 },
            { text: '│  Gold extends trend?          78%   ▲+6  │', type: 'output', delay: 30 },
            { text: '│  Risk regime shift this week? 61%   →0   │', type: 'output', delay: 30 },
            { text: '└──────────────────────────────────────────┘', type: 'output', delay: 30 },
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
        .replace(/▼ -[\d.]+%?/g, '<span style="color:var(--accent-red)">$&</span>')
        .replace(/▼-\d+/g, '<span style="color:var(--accent-red)">$&</span>')
        .replace(/→0/g, '<span style="color:var(--text-tertiary)">$&</span>')
        .replace(/(RSI \d+)/g, '<span style="color:var(--accent-blue)">$1</span>')
        .replace(/(█+)/g, '<span style="color:var(--accent-green)">$1</span>')
        .replace(/(░+)/g, '<span style="color:var(--bg-tertiary)">$1</span>')
        .replace(/(\d+%)\s/g, '<span style="color:var(--accent-cyan)">$1</span> ')
        .replace(/([\d,]+\.\d+|\$[\d,]+)/g, '<span style="color:var(--text-primary);font-weight:500">$1</span>');
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
