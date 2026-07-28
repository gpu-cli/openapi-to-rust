#!/usr/bin/env bash
# Download OpenAPI specs for top integration providers
set -euo pipefail

DIR="$(cd "$(dirname "$0")" && pwd)"
cd "$DIR"

download() {
    local name="$1" url="$2" filename="$3"
    if [ -f "$filename" ]; then
        echo "  [skip] $name (already exists)"
        return
    fi
    echo "  [downloading] $name"
    if curl -sL --fail -o "$filename" "$url" 2>/dev/null; then
        echo "  [ok] $name"
    else
        echo "  [FAIL] $name"
        rm -f "$filename"
    fi
}

echo "=== Downloading OpenAPI specs ==="

# --- Developer Tools & Git Platforms ---
download "github" \
    "https://raw.githubusercontent.com/github/rest-api-description/main/descriptions/api.github.com/api.github.com.json" \
    "github.json"

download "gitlab" \
    "https://gitlab.com/gitlab-org/gitlab/-/raw/master/doc/api/openapi/openapi_v2.yaml" \
    "gitlab.yaml"

download "bitbucket" \
    "https://dac-static.atlassian.com/cloud/bitbucket/swagger.v3.json" \
    "bitbucket.json"

# --- Issue Tracking & Project Management ---
download "linear" \
    "https://raw.githubusercontent.com/linearapp/linear/master/packages/sdk/src/openapi.json" \
    "linear.json"

download "jira-v3" \
    "https://dac-static.atlassian.com/cloud/jira/platform/swagger-v3.v3.json" \
    "jira.json"

download "asana" \
    "https://raw.githubusercontent.com/Asana/openapi/master/defs/asana_oas.yaml" \
    "asana.yaml"

download "clickup" \
    "https://raw.githubusercontent.com/nicojones/clickup-openapi/main/openapi.json" \
    "clickup.json"

download "trello" \
    "https://developer.atlassian.com/cloud/trello/swagger.v3.json" \
    "trello.json"

# --- Communication ---
download "slack-web-api" \
    "https://raw.githubusercontent.com/slackapi/slack-api-specs/master/web-api/slack_web_openapi_v2.json" \
    "slack.json"

download "discord" \
    "https://raw.githubusercontent.com/discord/discord-api-spec/main/specs/openapi.json" \
    "discord.json"

download "twilio" \
    "https://raw.githubusercontent.com/twilio/twilio-oai/main/spec/json/twilio_api_v2010.json" \
    "twilio.json"

download "sendgrid" \
    "https://raw.githubusercontent.com/sendgrid/sendgrid-oai/main/oai_stoplight.json" \
    "sendgrid.json"

download "telegram-bot" \
    "https://raw.githubusercontent.com/nickolasburr/openapi-telegram-bot-api/master/openapi.yaml" \
    "telegram.yaml"

# --- Knowledge & Docs ---
download "notion" \
    "https://raw.githubusercontent.com/readmeio/oas-examples/main/3.1/json/notion.json" \
    "notion.json"

download "confluence" \
    "https://dac-static.atlassian.com/cloud/confluence/swagger.v3.json" \
    "confluence.json"

# --- Monitoring & Incident Management ---
download "pagerduty" \
    "https://raw.githubusercontent.com/PagerDuty/api-schema/main/reference/REST/openapiv3.json" \
    "pagerduty.json"

download "sentry" \
    "https://raw.githubusercontent.com/getsentry/sentry/master/src/sentry/apidocs/openapi-derefed.json" \
    "sentry.json"

download "datadog" \
    "https://raw.githubusercontent.com/DataDog/datadog-api-client-go/master/.generator/schemas/v2/openapi.yaml" \
    "datadog-v2.yaml"

download "grafana" \
    "https://raw.githubusercontent.com/grafana/grafana/main/public/openapi3.json" \
    "grafana.json"

download "opsgenie" \
    "https://raw.githubusercontent.com/opsgenie/opsgenie-oas/master/swagger.json" \
    "opsgenie.json"

# --- CI/CD & Deployment ---
download "vercel" \
    "https://openapi.vercel.sh" \
    "vercel.json"

download "netlify" \
    "https://open-api.netlify.com/api" \
    "netlify.json"

download "cloudflare" \
    "https://raw.githubusercontent.com/cloudflare/api-schemas/main/openapi.json" \
    "cloudflare.json"

download "render" \
    "https://api-docs.render.com/openapi/6140fb30845b1c0045a25e35" \
    "render.json"

download "fly" \
    "https://raw.githubusercontent.com/superfly/fly-openapi/main/openapi/machines/openapi3.json" \
    "fly.json"

download "digitalocean" \
    "https://api-engineering.nyc3.cdn.digitaloceanspaces.com/spec-ci/DigitalOcean-public.v2.yaml" \
    "digitalocean.yaml"

download "circleci" \
    "https://circleci.com/api/v2/openapi.json" \
    "circleci.json"

download "buildkite" \
    "https://raw.githubusercontent.com/buildkite/buildkite-api-spec/main/generated/openapi.json" \
    "buildkite.json"

# --- Payments & Commerce ---
download "stripe" \
    "https://raw.githubusercontent.com/stripe/openapi/master/openapi/spec3.json" \
    "stripe.json"

# --- Analytics & Feature Flags ---
download "posthog" \
    "https://raw.githubusercontent.com/PostHog/posthog/master/openapi/bundled_schema.json" \
    "posthog.json"

download "launchdarkly" \
    "https://app.launchdarkly.com/api/v2/openapi.json" \
    "launchdarkly.json"

# --- Cloud Infrastructure ---
download "hetzner" \
    "https://docs.hetzner.cloud/spec.json" \
    "hetzner.json"

download "linode" \
    "https://raw.githubusercontent.com/linode/linode-api-docs/development/openapi.yaml" \
    "linode.yaml"

download "vultr" \
    "https://www.vultr.com/api/v2/openapi.yaml" \
    "vultr.yaml"

# --- Security ---
download "snyk" \
    "https://api.snyk.io/rest/openapi" \
    "snyk.json"

# --- CRM & Support ---
download "hubspot-crm" \
    "https://api.hubspot.com/api-catalog-public/v1/apis/crm/v3/objects/contacts" \
    "hubspot-contacts.json"

download "zendesk-support" \
    "https://developer.zendesk.com/api-reference/ticketing/openapi.json" \
    "zendesk.json"

download "intercom" \
    "https://raw.githubusercontent.com/intercom/Intercom-OpenAPI/main/descriptions/2.10/api.intercom.io.json" \
    "intercom.json"

# --- Databases & BaaS ---
download "supabase" \
    "https://api.supabase.com/api/v1-json" \
    "supabase.json"

# --- AI/ML APIs ---
download "openai" \
    "https://raw.githubusercontent.com/openai/openai-openapi/master/openapi.yaml" \
    "openai.yaml"

download "anthropic" \
    "https://raw.githubusercontent.com/anthropics/anthropic-cookbook/main/misc/anthropic_openapi_spec.yaml" \
    "anthropic.yaml"

# --- Version Control & Code ---
download "gitea" \
    "https://gitea.com/swagger.v1.json" \
    "gitea.json"

# --- Email ---
download "mailgun" \
    "https://raw.githubusercontent.com/mailgun/mailgun-openapi/master/spec/openapi.json" \
    "mailgun.json"

download "resend" \
    "https://raw.githubusercontent.com/resend/resend-openapi/main/resend.yaml" \
    "resend.yaml"

# --- Storage & Files ---
download "dropbox" \
    "https://raw.githubusercontent.com/nicholasgasior/dropbox-openapi-spec/master/openapi.yaml" \
    "dropbox.yaml"

download "box" \
    "https://raw.githubusercontent.com/box/box-openapi/main/openapi.json" \
    "box.json"

# --- Scheduling ---
download "cal-com" \
    "https://api.cal.com/v2/docs-json" \
    "cal-com.json"

# --- DNS & Domains ---
download "cloudflare-dns" \
    "https://raw.githubusercontent.com/cloudflare/api-schemas/main/openapi.json" \
    "cloudflare-dns.json"

# --- Community Platforms ---
download "storyden" \
    "https://raw.githubusercontent.com/Southclaws/storyden/main/api/openapi.yaml" \
    "storyden.yaml"

# --- Misc DevTools ---
download "railway" \
    "https://docs.railway.com/reference/public-api-spec.json" \
    "railway.json"

echo ""
echo "=== Download complete ==="
ls -1 *.{json,yaml,yml} 2>/dev/null | wc -l | xargs -I{} echo "Downloaded {} specs"
