FROM scratch

ARG BINARY

COPY --chmod=755 ${BINARY} /cachebolt

ENTRYPOINT ["/cachebolt"]
