package loadfeed

import (
	"encoding/json"
	"fmt"
	"net/http"
)

type Feed struct {
	UserID string
	Posts  []string
}

type FeedError struct {
	Kind   string // "network" or "decode"
	Status int
	Reason string
}

func (e FeedError) Error() string {
	if e.Kind == "network" {
		return fmt.Sprintf("network error: status %d", e.Status)
	}
	return "decode error: " + e.Reason
}

func LoadFeed(client *http.Client, userID string) (Feed, error) {
	userResp, err := client.Get(fmt.Sprintf("/api/users/%s", userID))
	if err != nil {
		return Feed{}, FeedError{Kind: "decode", Reason: err.Error()}
	}
	defer userResp.Body.Close()
	if userResp.StatusCode >= 400 {
		return Feed{}, FeedError{Kind: "network", Status: userResp.StatusCode}
	}

	postsResp, err := client.Get(fmt.Sprintf("/api/users/%s/posts", userID))
	if err != nil {
		return Feed{}, FeedError{Kind: "decode", Reason: err.Error()}
	}
	defer postsResp.Body.Close()
	if postsResp.StatusCode >= 400 {
		return Feed{}, FeedError{Kind: "network", Status: postsResp.StatusCode}
	}

	var postsData struct {
		Titles []string `json:"titles"`
	}
	if err := json.NewDecoder(postsResp.Body).Decode(&postsData); err != nil {
		return Feed{}, FeedError{Kind: "decode", Reason: err.Error()}
	}
	return Feed{UserID: userID, Posts: postsData.Titles}, nil
}
