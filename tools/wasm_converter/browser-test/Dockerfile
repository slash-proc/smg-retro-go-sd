FROM mcr.microsoft.com/playwright:v1.63.0-noble
WORKDIR /runner
COPY package.json ./
RUN npm install --ignore-scripts --no-audit --no-fund
COPY playwright.config.mjs ./
COPY tests ./tests
ENTRYPOINT ["npm", "test"]
