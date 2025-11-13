-- WRK Lua script for POST requests with valid user data

wrk.method = "POST"
wrk.headers["Content-Type"] = "application/json"

-- Valid user payload
wrk.body = [[{
  "email": "loadtest@example.com",
  "username": "loadtest_user",
  "name": {
    "first": "Load",
    "last": "Test"
  },
  "age": 25,
  "roles": ["user"]
}]]

-- Optional: Track response codes
local responses = {}

function response(status, headers, body)
    responses[status] = (responses[status] or 0) + 1
end

function done(summary, latency, requests)
    io.write("Status Code Distribution:\n")
    for status, count in pairs(responses) do
        io.write(string.format("  %d: %d\n", status, count))
    end
end

