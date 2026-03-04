// ===== COPY TO CLIPBOARD =====
function copyInstall() {
    const text = document.getElementById('install-cmd').textContent;
    copyToClipboard(text);
}

function copyCode(button) {
    const codeBlock = button.previousElementSibling;
    const text = codeBlock.textContent;
    copyToClipboard(text);
    
    // Visual feedback
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
        // Fallback for older browsers
        const textArea = document.createElement('textarea');
        textArea.value = text;
        textArea.style.position = 'fixed';
        textArea.style.left = '-999999px';
        document.body.appendChild(textArea);
        textArea.select();
        try {
            document.execCommand('copy');
        } catch (err) {
            console.error('Copy failed:', err);
        }
        document.body.removeChild(textArea);
    }
}

// ===== TERMINAL TYPING ANIMATION =====
const terminalOutput = [
    '$ pftui',
    '',
    'pftui v0.3.0',
    '',
    '┌─ Portfolio Overview ─────────────────────┐',
    '│ $367.8k  +1.2%                          │',
    '│                                         │',
    '│ Cash      49%  ████████████░░░░░░░░░   │',
    '│ Comd      31%  ███████░░░░░░░░░░░░░░   │',
    '│ Crypto    20%  ████░░░░░░░░░░░░░░░░░   │',
    '└─────────────────────────────────────────┘',
    '',
    '📊 Live prices  📈 Charts  🌍 Macro data',
];

let lineIndex = 0;
let charIndex = 0;

function typeTerminal() {
    const terminal = document.getElementById('terminal');
    
    if (lineIndex < terminalOutput.length) {
        const currentLine = terminalOutput[lineIndex];
        
        if (charIndex <= currentLine.length) {
            const lineElement = terminal.children[lineIndex];
            if (!lineElement) {
                const newLine = document.createElement('div');
                newLine.className = 'terminal-line';
                terminal.insertBefore(newLine, terminal.querySelector('.terminal-cursor'));
            }
            
            terminal.children[lineIndex].textContent = currentLine.slice(0, charIndex);
            charIndex++;
            setTimeout(typeTerminal, 20);
        } else {
            lineIndex++;
            charIndex = 0;
            setTimeout(typeTerminal, 100);
        }
    }
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
            target.scrollIntoView({
                behavior: 'smooth',
                block: 'start'
            });
        }
    });
});

// ===== INIT ON LOAD =====
window.addEventListener('DOMContentLoaded', () => {
    // Start terminal animation
    setTimeout(typeTerminal, 500);
    
    // Observe fade-in elements
    document.querySelectorAll('.fade-in').forEach(el => {
        observer.observe(el);
    });
    
    // Observe feature cards
    document.querySelectorAll('.feature-card').forEach(el => {
        observer.observe(el);
    });
});

// ===== PERFORMANCE: Lazy load images =====
if ('loading' in HTMLImageElement.prototype) {
    const images = document.querySelectorAll('img[loading="lazy"]');
    images.forEach(img => {
        img.src = img.dataset.src || img.src;
    });
} else {
    // Fallback for browsers that don't support lazy loading
    const script = document.createElement('script');
    script.src = 'https://cdnjs.cloudflare.com/ajax/libs/lazysizes/5.3.2/lazysizes.min.js';
    document.body.appendChild(script);
}
