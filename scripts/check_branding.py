#!/usr/bin/env python3
import os
import sys
import argparse

# Configurações
FORBIDDEN_WORDS = ["nexus"]
IGNORE_DIRS = [".git", "node_modules", "target", "logs", "dist", "pkg"]
IGNORE_FILES = ["check_branding.py"] # Don't check yourself

def check_branding(root_dir):
    found_violations = 0
    
    for root, dirs, files in os.walk(root_dir):
        # Filtrar diretórios ignorados
        dirs[:] = [d for d in dirs if d not in IGNORE_DIRS]
        
        for file in files:
            if file in IGNORE_FILES:
                continue
                
            file_path = os.path.join(root, file)
            
            # Pular arquivos binários básicos
            if file_path.endswith((".wasm", ".png", ".jpg", ".jpeg", ".ico", ".aeb")):
                continue
                
            try:
                with open(file_path, "r", encoding="utf-8") as f:
                    content = f.read().lower()
                    for word in FORBIDDEN_WORDS:
                        if word in content:
                            print(f"❌ Violation found: '{word}' in {file_path}")
                            found_violations += 1
            except (UnicodeDecodeError, PermissionError):
                # Skip files that cannot be read as text
                continue
                
    return found_violations

if __name__ == "__main__":
    parser = argparse.ArgumentParser(description="Aetheris Branding Guard — Check for forbidden internal terms.")
    parser.add_argument("path", nargs="?", default=".", help="Root directory to scan (default: current)")
    args = parser.parse_args()
    
    violations = check_branding(args.path)
    
    if violations > 0:
        print(f"\n🚨 Total branding violations: {violations}")
        sys.exit(1)
    else:
        print("✅ Branding check passed. No internal terms found.")
        sys.exit(0)
