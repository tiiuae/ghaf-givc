#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2025-2026 TII (SSRC) and the Ghaf contributors
# SPDX-License-Identifier: Apache-2.0

set -euo pipefail

# Default target year and path
TARGET_YEAR=$(date +%Y)
TARGET_PATH="."

# Parse command line arguments
while [[ $# -gt 0 ]]; do
    case $1 in
        -y|--year)
            TARGET_YEAR="$2"
            shift 2
            ;;
        -p|--path)
            TARGET_PATH="$2"
            shift 2
            ;;
        -h|--help)
            echo "Usage: $0 [OPTIONS]"
            echo "Update copyright years in project files"
            echo ""
            echo "OPTIONS:"
            echo "  -y, --year YEAR    Target year to update to (default: current year)"
            echo "  -p, --path PATH    Target file or directory to process (default: .)"
            echo "  -h, --help         Show this help message"
            exit 0
            ;;
        *)
            echo "Unknown option: $1" >&2
            echo "Use --help for usage information" >&2
            exit 1
            ;;
    esac
done

# Warn user and ask for confirmation
echo "This script will update copyright years in project files to $TARGET_YEAR.
This operation cannot be undone automatically, and may require manual fixes."
read -rp "Do you want to continue? (y/N): " confirm
if [[ ! "$confirm" =~ ^[Yy]$ ]]; then
    echo "Aborting."
    exit 0
fi

# Validate year
if ! [[ "$TARGET_YEAR" =~ ^[0-9]{4}$ ]]; then
    echo "Error: Invalid year '$TARGET_YEAR'. Must be a 4-digit year." >&2
    exit 1
fi

echo "Updating copyright years to $TARGET_YEAR..."

# Get the project root directory (assuming script is in scripts/ subdirectory)
PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)" || {
    echo "Error: Cannot determine project root directory" >&2
    exit 1
}
cd "$PROJECT_ROOT" || {
    echo "Error: Cannot change to project root directory" >&2
    exit 1
}

# File patterns to include
FILE_PATTERNS=(-name "*.nix" -o -name "*.go" -o -name "*.proto" -o -name "*.rs" -o -name "*.md" -o -name "*.yml" -o -name "*.sh" -o -name "*.envrc")

# Function to get appropriate copyright header for file type
get_copyright_header() {
    local file="$1"
    local start_year="$2"
    local file_ext="${file##*.}"

    local year_range
    if [[ "$start_year" == "$TARGET_YEAR" ]]; then
        year_range="$TARGET_YEAR"
    else
        year_range="$start_year-$TARGET_YEAR"
    fi

    case "$file_ext" in
        "nix"|"envrc")
            echo "# SPDX-FileCopyrightText: $year_range TII (SSRC) and the Ghaf contributors
# SPDX-License-Identifier: Apache-2.0"
            ;;
        "go"|"proto"|"rs")
            echo "// SPDX-FileCopyrightText: $year_range TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0"
            ;;
        "yml"|"yaml")
            echo "# SPDX-FileCopyrightText: $year_range TII (SSRC) and the Ghaf contributors
# SPDX-License-Identifier: Apache-2.0"
            ;;
        "md")
            echo "<!--
    SPDX-FileCopyrightText: $year_range TII (SSRC) and the Ghaf contributors
    SPDX-License-Identifier: CC-BY-SA-4.0
-->"
            ;;
        "sh")
            # Check if it's a shell script with shebang
            if head -1 "$file" 2>/dev/null | grep -q "^#!"; then
                local shebang
                shebang=$(head -1 "$file")
                echo "$shebang
# SPDX-FileCopyrightText: $year_range TII (SSRC) and the Ghaf contributors
# SPDX-License-Identifier: Apache-2.0"
            else
                echo "# SPDX-FileCopyrightText: $year_range TII (SSRC) and the Ghaf contributors
# SPDX-License-Identifier: Apache-2.0"
            fi
            ;;
        *)
            echo "# SPDX-FileCopyrightText: $year_range TII (SSRC) and the Ghaf contributors
# SPDX-License-Identifier: Apache-2.0"
            ;;
    esac
}

# Function to extract start year from copyright line
extract_start_year() {
    local line="$1"
    # Extract year from patterns like "2024-2025" or "2024"
    if [[ "$line" =~ ([0-9]{4})-[0-9]{4} ]]; then
        echo "${BASH_REMATCH[1]}"
    elif [[ "$line" =~ ([0-9]{4})[[:space:]]+TII ]]; then
        echo "${BASH_REMATCH[1]}"
    else
        echo "$TARGET_YEAR"  # Default to current year if can't parse
    fi
}

# Function to process a single file
process_file() {
    local file="$1"
    local file_count="$2"
    local total_files="$3"

    if [[ ! -f "$file" ]] || [[ ! -w "$file" ]]; then
        echo "Skipping $file (not writable)"
        return 0
    fi

    if [[ $((file_count % 10)) -eq 0 ]]; then
        echo "Progress: $file_count/$total_files files processed"
    fi

    # Check if file has any copyright header
    local has_copyright=false
    local has_spdx=false
    local start_year="$TARGET_YEAR"

    # Look for copyright in first 5 lines
    local line_count=0
    while IFS= read -r line && [[ $line_count -lt 5 ]]; do
        ((++line_count))
        if [[ "$line" =~ (Copyright|SPDX-FileCopyrightText) ]]; then
            has_copyright=true
            start_year=$(extract_start_year "$line")

            if [[ "$line" =~ SPDX-FileCopyrightText ]]; then
                has_spdx=true
                # Check if already has target year
                if [[ "$line" =~ -${TARGET_YEAR}[[:space:]] ]] || [[ "$line" =~ ^[^0-9]*${TARGET_YEAR}[[:space:]]+TII ]]; then
                    echo "  $file: Already up to date"
                    return 0
                fi
            fi
            break
        fi
    done < "$file"

    # Determine action needed
    if [[ "$has_copyright" == false ]]; then
        # No header found - add new SPDX header
        echo "  $file: Adding copyright header"
        local new_header
        local start_year
        start_year=$(git log --date=format:%Y --pretty=format:%ad -- "$file" | tail -n 1)
        new_header=$(get_copyright_header "$file" "${start_year:-$TARGET_YEAR}")

        # Handle shell scripts with shebang specially
        if [[ "${file##*.}" == "sh" ]] && head -1 "$file" 2>/dev/null | grep -q "^#!"; then
            local temp_file
            temp_file=$(mktemp)
            local shebang
            shebang=$(head -1 "$file")
            {
                echo "$shebang"
                echo "$new_header"
                echo ""
                tail -n +2 "$file"
            } > "$temp_file" && mv "$temp_file" "$file"
        else
            local temp_file
            temp_file=$(mktemp)
            {
                echo "$new_header"
                echo ""
                cat "$file"
            } > "$temp_file" && mv "$temp_file" "$file"
        fi

    elif [[ "$has_spdx" == false ]]; then
        # Has old format copyright - replace with SPDX using sed
        echo "  $file: Converting to SPDX format (preserving year $start_year)"

        local year_range
        if [[ "$start_year" == "$TARGET_YEAR" ]]; then
            year_range="$TARGET_YEAR"
        else
            year_range="$start_year-$TARGET_YEAR"
        fi

        local file_ext="${file##*.}"
        case "$file_ext" in
            "go"|"proto"|"rs")
                sed -i -E "s|^([[:space:]]*)// Copyright ([0-9]{4}(-[0-9]{4})?) TII.*|\\1// SPDX-FileCopyrightText: $year_range TII (SSRC) and the Ghaf contributors|" "$file"
                ;;
            "sh"|"nix"|"envrc"|"yml"|"yaml")
                sed -i -E "s|^([[:space:]]*)# Copyright ([0-9]{4}(-[0-9]{4})?) TII.*|\\1# SPDX-FileCopyrightText: $year_range TII (SSRC) and the Ghaf contributors|" "$file"
                ;;
            "md")
                # For markdown, we need to handle HTML comments differently
                sed -i -E "s|<!--[[:space:]]*Copyright ([0-9]{4}(-[0-9]{4})?) TII.*-->|<!--\\n    SPDX-FileCopyrightText: $year_range TII (SSRC) and the Ghaf contributors\\n    SPDX-License-Identifier: CC-BY-SA-4.0\\n-->|" "$file"
                ;;
        esac

    else
        # Has SPDX format - just update the year
        echo "  $file: Updating SPDX year range (from $start_year to $start_year-$TARGET_YEAR)"

        if [[ "$start_year" == "$TARGET_YEAR" ]]; then
            echo "  $file: Year already current, no update needed"
            return 0
        fi

        # Update the year in place
        local year_pattern="$start_year-$TARGET_YEAR"
        sed -i -E "s|(SPDX-FileCopyrightText: )$start_year([[:space:]]+TII)|\1$year_pattern\2|g" "$file"
        sed -i -E "s|(SPDX-FileCopyrightText: )$start_year-[0-9]{4}([[:space:]]+TII)|\1$year_pattern\2|g" "$file"
    fi

    return 0
}

# Validate target path
if [[ ! -e "$TARGET_PATH" ]]; then
    echo "Error: Target path '$TARGET_PATH' does not exist." >&2
    exit 1
fi

# Main processing
echo "Scanning for files to process in: $TARGET_PATH"
if [[ -f "$TARGET_PATH" ]]; then
    # Single file processing
    all_files=("$TARGET_PATH")
else
    # Directory processing
    mapfile -d '' all_files < <(find "$TARGET_PATH" -type f \( "${FILE_PATTERNS[@]}" \) -not -path "*/.git/*" -not -path "./api/*" -print0)
fi

total_files=${#all_files[@]}
echo "Found $total_files files to process"

if [[ $total_files -eq 0 ]]; then
    echo "No files found to process."
    exit 0
fi

# Process each file
files_processed=0
files_updated=0

echo "Starting file processing..."

for file in "${all_files[@]}"; do
    # Skip empty entries
    if [[ -z "$file" ]]; then
        continue
    fi

    ((++files_processed))
    echo "Processing file $files_processed: $file"

    if process_file "$file" "$files_processed" "$total_files"; then
        ((++files_updated))
    fi

done

echo ""
echo "Processing completed!"
echo "Files processed: $files_processed"
echo "Files updated: $files_updated"
echo ""
echo "All copyright headers are now in SPDX format with year range ending in $TARGET_YEAR."
echo "Please review the changes before committing."
