# Test cargo-dist release build by creating a PR
dist-test-pr:
    #!/usr/bin/env bash
    set -euo pipefail

    # Checkout new branch
    BRANCH_NAME="dist-test-$(date +%Y%m%d-%H%M%S)"
    echo "Creating new branch: $BRANCH_NAME"
    git checkout -b "$BRANCH_NAME"

    # Patch dist-workspace.toml to set pr-run-mode = "upload"
    echo "Patching dist-workspace.toml..."
    sed -i.bak 's/pr-run-mode = "plan"/pr-run-mode = "upload"/' dist-workspace.toml
    rm -f dist-workspace.toml.bak

    # Run dist generate to generate updated workflow
    echo "Running cargo dist generate..."
    dist generate

    # Check if there are changes to commit
    if ! git diff --quiet; then
        echo "Committing changes..."
        git add .
        git commit -m "test: cargo-dist release artifact build"

        echo "Pushing branch and creating PR..."
        git push -u origin "$BRANCH_NAME"

        gh pr create \
            --title "test: cargo-dist release artifact build" \
            --body "Test cargo-dist release build process."

        echo "PR created successfully!"
    else
        echo "No changes to commit. Cleaning up branch..."
        git checkout -
        git branch -D "$BRANCH_NAME"
    fi
