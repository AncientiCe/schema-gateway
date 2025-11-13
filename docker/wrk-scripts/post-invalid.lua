-- WRK Lua script for POST requests with invalid user data
-- Tests permissive mode (forward_on_error: true)

wrk.method = "POST"
wrk.headers["Content-Type"] = "application/json"

-- Invalid user payload (missing required fields)
wrk.body = [[{
  "username": "invalid_user"
}]]

-- Track response codes and error headers
local responses = {}
local error_headers = 0

function response(status, headers, body)
    responses[status] = (responses[status] or 0) + 1
    
    -- Check for X-Gateway-Error header
    if headers["X-Gateway-Error"] or headers["x-gateway-error"] then
        error_headers = error_headers + 1
    end
end

function done(summary, latency, requests)
    io.write("Status Code Distribution:\n")
    for status, count in pairs(responses) do
        io.write(string.format("  %d: %d\n", status, count))
    end
    io.write(string.format("\nRequests with X-Gateway-Error header: %d\n", error_headers))
end

