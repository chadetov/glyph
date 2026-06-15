package slugify

import (
	"regexp"
	"strings"
)

var (
	nonAlphanumeric = regexp.MustCompile(`[^a-z0-9]+`)
	edgeDashes      = regexp.MustCompile(`^-+|-+$`)
)

func Slugify(s string) string {
	lower := strings.ToLower(s)
	dashed := nonAlphanumeric.ReplaceAllString(lower, "-")
	return edgeDashes.ReplaceAllString(dashed, "")
}
